//! Vigía de la BD: avisa cuando otra conexión ha escrito (`PRAGMA data_version`).

use crate::db;
use rusqlite::Connection;
use std::path::PathBuf;
use std::time::Duration;

/// Cada cuánto se mira la versión de datos.
const INTERVALO: Duration = Duration::from_millis(700);

/// Arranca un hilo con conexión propia que llama a `al_cambiar` cuando la BD cambia desde
/// otra conexión.
pub fn arrancar(ruta_bd: PathBuf, al_cambiar: impl Fn() + Send + 'static) {
    let hilo = std::thread::Builder::new()
        .name("migas-vigia".into())
        .spawn(move || {
            let conn = match db::abrir(&ruta_bd) {
                Ok(conn) => conn,
                Err(error) => {
                    eprintln!("migas vigia: no se pudo abrir la BD: {error}");
                    return;
                }
            };
            let mut ultima = version(&conn);
            loop {
                std::thread::sleep(INTERVALO);
                let actual = version(&conn);
                if actual != ultima {
                    ultima = actual;
                    al_cambiar();
                }
            }
        });
    if let Err(error) = hilo {
        eprintln!("migas vigia: no se pudo arrancar el hilo: {error}");
    }
}

fn version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "data_version", |fila| fila.get(0))
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_version_cambia_con_otra_conexion() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let conn = db::abrir(&ruta).expect("BD");
        let otra = db::abrir(&ruta).expect("otra BD");
        let antes = version(&conn);
        otra.execute("INSERT INTO meta (clave, valor) VALUES ('prueba', '1')", [])
            .expect("escribe");
        assert_ne!(version(&conn), antes);
    }
}
