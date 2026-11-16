//! Base de datos SQLite de migas: apertura, PRAGMAs, migración, meta y purga de papelera.

use crate::config::dir_datos;
use crate::error::Resultado;
use chrono::{DateTime, Duration, Local};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::path::Path;

/// Abre (o crea) la BD, aplica los PRAGMAs y ejecuta la migración pendiente.
pub fn abrir(ruta: &Path) -> Resultado<Connection> {
    if let Some(carpeta) = ruta.parent()
        && !carpeta.as_os_str().is_empty()
    {
        std::fs::create_dir_all(carpeta)?;
    }
    let conn = Connection::open(ruta)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;\
         PRAGMA foreign_keys=ON;\
         PRAGMA busy_timeout=4000;\
         PRAGMA synchronous=NORMAL;",
    )?;
    migrar(&conn)?;
    Ok(conn)
}

/// Abre la BD por defecto: `dir_datos()/migas.db`.
pub fn abrir_defecto() -> Resultado<Connection> {
    abrir(&dir_datos().join("migas.db"))
}

/// Empieza una transacción `IMMEDIATE`: toma el bloqueo de escritura antes de leer, así una
/// lectura seguida de escritura no falla con `SQLITE_BUSY` al intentar subir de bloqueo.
pub fn transaccion_inmediata(conn: &Connection) -> Resultado<Transaction<'_>> {
    Ok(Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    )?)
}

/// Aplica `esquema.sql` si `user_version < 1`, todo dentro de una transacción.
fn migrar(conn: &Connection) -> Resultado<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |fila| fila.get(0))?;
    if version < 1 {
        let tx = transaccion_inmediata(conn)?;
        tx.execute_batch(include_str!("esquema.sql"))?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// Borra de verdad las entradas de papelera con más de 30 días (y su fila de FTS).
pub fn purgar_papelera(conn: &Connection, ahora: DateTime<Local>) -> Resultado<usize> {
    let corte = (ahora - Duration::days(30)).to_rfc3339();
    let tx = transaccion_inmediata(conn)?;
    let ids: Vec<i64> = {
        let mut consulta = tx.prepare(
            "SELECT id FROM entrada WHERE borrada IS NOT NULL AND julianday(borrada) < julianday(?1)",
        )?;
        let filas = consulta.query_map([&corte], |fila| fila.get(0))?;
        filas.collect::<Result<Vec<i64>, _>>()?
    };
    let borradas = borrar_entradas(&tx, &ids)?;
    tx.commit()?;
    Ok(borradas)
}

/// Borra de verdad las entradas de `ids` y sus filas de `entrada_fts`, dentro de la
/// transacción que le pasan. FTS5 no tiene claves foráneas, así que se borra a mano; las
/// demás tablas hijas caen por `ON DELETE CASCADE`. Los recursos que se quedan sin ninguna
/// entrada (ni viva ni en papelera) se borran también, con sus `recurso_entidad`.
/// Devuelve cuántas entradas borró.
pub fn borrar_entradas(tx: &Transaction<'_>, ids: &[i64]) -> Resultado<usize> {
    let mut claves: Vec<String> = Vec::new();
    for id in ids {
        let clave: Option<String> = tx
            .query_row("SELECT recurso FROM entrada WHERE id = ?1", [id], |fila| {
                fila.get(0)
            })
            .optional()?
            .flatten();
        if let Some(clave) = clave
            && !claves.contains(&clave)
        {
            claves.push(clave);
        }
        tx.execute("DELETE FROM entrada_fts WHERE rowid = ?1", [id])?;
    }
    let mut borradas = 0;
    for id in ids {
        borradas += tx.execute("DELETE FROM entrada WHERE id = ?1", [id])?;
    }
    for clave in &claves {
        tx.execute(
            "DELETE FROM recurso WHERE clave = ?1
               AND NOT EXISTS (SELECT 1 FROM entrada WHERE recurso = ?1)",
            [clave],
        )?;
    }
    Ok(borradas)
}

/// Lee un valor de `meta`. `None` si no existe.
pub fn meta_leer(conn: &Connection, clave: &str) -> Resultado<Option<String>> {
    Ok(conn
        .query_row("SELECT valor FROM meta WHERE clave = ?1", [clave], |fila| {
            fila.get(0)
        })
        .optional()?)
}

/// Escribe (crea o sustituye) un valor de `meta`.
pub fn meta_escribir(conn: &Connection, clave: &str, valor: &str) -> Resultado<()> {
    conn.execute(
        "INSERT INTO meta (clave, valor) VALUES (?1, ?2)
         ON CONFLICT(clave) DO UPDATE SET valor = excluded.valor",
        [clave, valor],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn bd(dir: &Path) -> Connection {
        abrir(&dir.join("migas.db")).expect("abre la BD de prueba")
    }

    #[test]
    fn abre_migra_y_reabre_sin_romper() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("sub").join("migas.db");
        let conn = abrir(&ruta).expect("abre");
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |fila| fila.get(0))
            .expect("user_version");
        assert_eq!(version, 1);
        let foraneas: i64 = conn
            .pragma_query_value(None, "foreign_keys", |fila| fila.get(0))
            .expect("foreign_keys");
        assert_eq!(foraneas, 1);
        let espera: i64 = conn
            .pragma_query_value(None, "busy_timeout", |fila| fila.get(0))
            .expect("busy_timeout");
        assert_eq!(espera, 4000);
        let sincronia: i64 = conn
            .pragma_query_value(None, "synchronous", |fila| fila.get(0))
            .expect("synchronous");
        assert_eq!(sincronia, 1, "NORMAL");
        let modo: String = conn
            .pragma_query_value(None, "journal_mode", |fila| fila.get(0))
            .expect("journal_mode");
        assert_eq!(modo.to_lowercase(), "wal");
        let tablas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'entrada'",
                [],
                |fila| fila.get(0),
            )
            .expect("tabla entrada");
        assert_eq!(tablas, 1);
        drop(conn);

        let conn = abrir(&ruta).expect("reabre");
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |fila| fila.get(0))
            .expect("user_version");
        assert_eq!(version, 1);
    }

    #[test]
    fn meta_ida_y_vuelta() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        assert_eq!(meta_leer(&conn, "ultima_extension").expect("lee"), None);
        meta_escribir(&conn, "ultima_extension", "2026-11-04T10:00:00+01:00").expect("escribe");
        assert_eq!(
            meta_leer(&conn, "ultima_extension").expect("lee"),
            Some("2026-11-04T10:00:00+01:00".to_string())
        );
        meta_escribir(&conn, "ultima_extension", "2026-11-05T10:00:00+01:00").expect("reescribe");
        assert_eq!(
            meta_leer(&conn, "ultima_extension").expect("lee"),
            Some("2026-11-05T10:00:00+01:00".to_string())
        );
    }

    #[test]
    fn purga_solo_lo_borrado_hace_mas_de_30_dias() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ahora = Local.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap();
        let vieja = (ahora - Duration::days(31)).to_rfc3339();
        let reciente = (ahora - Duration::days(5)).to_rfc3339();
        for (titulo, borrada) in [
            ("vieja", Some(&vieja)),
            ("reciente", Some(&reciente)),
            ("viva", None),
        ] {
            conn.execute(
                "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo, borrada)
                 VALUES ('2026-10-01', '2026-10-01T10:00:00+02:00', '2026-10-01T10:00:00+02:00',
                         'texto', 'cli', 'texto', ?1, ?2)",
                rusqlite::params![titulo, borrada],
            )
            .expect("inserta entrada");
            let id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO entrada_fts (rowid, titulo, url, seleccion, notas, extra)
                 VALUES (?1, ?2, '', '', '', '')",
                rusqlite::params![id, titulo],
            )
            .expect("inserta FTS");
        }

        let purgadas = purgar_papelera(&conn, ahora).expect("purga");
        assert_eq!(purgadas, 1);
        let quedan: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada", [], |fila| fila.get(0))
            .expect("cuenta");
        assert_eq!(quedan, 2);
        let fts: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada_fts", [], |fila| fila.get(0))
            .expect("cuenta FTS");
        assert_eq!(fts, 2);
        // La viva sigue ahí y la reciente también.
        let vivas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entrada WHERE borrada IS NULL OR titulo = 'reciente'",
                [],
                |fila| fila.get(0),
            )
            .expect("cuenta vivas");
        assert_eq!(vivas, 2);
    }
}
