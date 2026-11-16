//! Todo lo que piden los comandos: días, listados, búsqueda FTS, detalle, notas, papelera,
//! enlaces manuales y estado de una URL para el host.

use crate::error::{Error, Resultado};
use crate::model::{
    Ajustes, Aparicion, Cambio, DetalleEntrada, DiaResumen, EntradaVista, Filtro, Fuente, Huella,
    Listado, Nota, Origen, RecursoVista, TipoEntrada, TipoRecurso, Trozo, Vista,
};
use crate::{captura, db, entidades, huella, recurso, relaciones, util};
use chrono::{DateTime, Local};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::Serialize;
use std::collections::HashMap;

/// Lo que el host responde al mensaje `estado`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstadoUrl {
    pub guardada_hoy: bool,
    pub id: Option<i64>,
    /// Días distintos en que se guardó.
    pub veces: u32,
    pub cambios: u32,
    pub relacionadas: u32,
}

/// Días con entradas (desc), con total y entradas con cambios.
pub fn listar_dias(
    conn: &Connection,
    desde: Option<&str>,
    hasta: Option<&str>,
) -> Resultado<Vec<DiaResumen>> {
    let mut consulta = conn.prepare(
        "SELECT e.dia, COUNT(*),
                COALESCE(SUM(CASE WHEN r.cambios IS NOT NULL THEN 1 ELSE 0 END), 0)
           FROM entrada e LEFT JOIN recurso r ON r.clave = e.recurso
          WHERE e.borrada IS NULL
            AND (?1 IS NULL OR e.dia >= ?1)
            AND (?2 IS NULL OR e.dia <= ?2)
          GROUP BY e.dia
          ORDER BY e.dia DESC",
    )?;
    let filas = consulta.query_map(params![desde, hasta], |fila| {
        Ok(DiaResumen {
            dia: fila.get(0)?,
            total: fila.get(1)?,
            cambios: fila.get(2)?,
        })
    })?;
    Ok(filas.collect::<Result<Vec<DiaResumen>, _>>()?)
}

/// Listado de la columna central. `total` cuenta sin aplicar el límite.
pub fn listar(conn: &Connection, filtro: &Filtro, ahora: DateTime<Local>) -> Resultado<Listado> {
    let vista = filtro.vista.unwrap_or(Vista::Dia);
    let dia = filtro.dia.clone().unwrap_or_else(|| hoy(ahora));
    let limite = i64::from(filtro.limite.unwrap_or(300));

    let mut condiciones: Vec<String> = Vec::new();
    let mut valores: Vec<Value> = Vec::new();
    match vista {
        Vista::Dia => {
            condiciones.push("e.dia = ?".into());
            valores.push(Value::Text(dia));
            condiciones.push("e.borrada IS NULL".into());
        }
        Vista::Cambios => {
            condiciones.push("e.borrada IS NULL".into());
            condiciones.push("e.recurso IS NOT NULL".into());
            condiciones.push("r.cambios IS NOT NULL".into());
            condiciones.push(
                "e.id = (SELECT MAX(e2.id) FROM entrada e2
                          WHERE e2.recurso = e.recurso AND e2.borrada IS NULL)"
                    .into(),
            );
        }
        Vista::Fijadas => {
            condiciones.push("e.borrada IS NULL".into());
            condiciones.push("e.fijada = 1".into());
        }
        Vista::Todo => condiciones.push("e.borrada IS NULL".into()),
        Vista::Papelera => condiciones.push("e.borrada IS NOT NULL".into()),
    }
    if !filtro.fuentes.is_empty() {
        let marcas = vec!["?"; filtro.fuentes.len()].join(", ");
        condiciones.push(format!("e.fuente IN ({marcas})"));
        for fuente in &filtro.fuentes {
            valores.push(Value::Text(fuente_como_str(*fuente).into()));
        }
    }
    let consulta_texto = filtro.texto.as_deref().and_then(consulta_fts);
    if let Some(consulta_texto) = &consulta_texto {
        condiciones
            .push("e.id IN (SELECT rowid FROM entrada_fts WHERE entrada_fts MATCH ?)".into());
        valores.push(Value::Text(consulta_texto.clone()));
    }
    let donde = condiciones.join(" AND ");

    let total: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM entrada e LEFT JOIN recurso r ON r.clave = e.recurso WHERE {donde}"
        ),
        params_from_iter(valores.iter()),
        |fila| fila.get(0),
    )?;

    let mut valores_pagina = valores.clone();
    valores_pagina.push(Value::Integer(limite));
    let ids: Vec<i64> = {
        let mut consulta = conn.prepare(&format!(
            "SELECT e.id FROM entrada e LEFT JOIN recurso r ON r.clave = e.recurso
              WHERE {donde} ORDER BY e.id DESC LIMIT ?"
        ))?;
        consulta
            .query_map(params_from_iter(valores_pagina.iter()), |fila| fila.get(0))?
            .collect::<Result<Vec<i64>, _>>()?
    };

    let fragmentos = match &consulta_texto {
        Some(consulta_texto) => fragmentos(conn, consulta_texto)?,
        None => HashMap::new(),
    };
    let mut entradas = Vec::with_capacity(ids.len());
    for id in ids {
        let mut entrada = entrada_vista_base(conn, id)?;
        entrada.relacionadas = relaciones::contar(conn, id)?;
        entrada.fragmento = fragmentos.get(&id).cloned();
        entradas.push(entrada);
    }
    Ok(Listado {
        entradas,
        total: u32::try_from(total).unwrap_or(u32::MAX),
    })
}

/// Vista de una entrada con el contador de relacionadas.
pub fn entrada_vista(conn: &Connection, id: i64) -> Resultado<EntradaVista> {
    let mut entrada = entrada_vista_base(conn, id)?;
    entrada.relacionadas = relaciones::contar(conn, id)?;
    Ok(entrada)
}

/// Fila completa de `entrada` + datos del recurso, tal como sale de la consulta.
struct FilaEntrada {
    id: i64,
    dia: String,
    creada: String,
    tipo: String,
    origen: String,
    fuente: String,
    url: Option<String>,
    titulo: String,
    clave: Option<String>,
    fijada: i64,
    capturas: i64,
    borrada: Option<String>,
    seleccion: Option<String>,
    estado: Option<String>,
    proyecto: Option<String>,
    app: Option<String>,
    cambios: Option<String>,
    notas: i64,
    ultima_nota: Option<String>,
}

/// Vista de una entrada sin calcular relacionadas (evita la recursión con `relaciones`).
pub(crate) fn entrada_vista_base(conn: &Connection, id: i64) -> Resultado<EntradaVista> {
    let fila = conn
        .query_row(
            "SELECT e.id, e.dia, e.creada, e.tipo, e.origen, e.fuente, e.url, e.titulo,
                    e.recurso, e.fijada, e.capturas, e.borrada, e.seleccion,
                    r.estado, r.proyecto, r.app, r.cambios,
                    (SELECT COUNT(*) FROM nota n WHERE n.entrada_id = e.id),
                    (SELECT n.texto FROM nota n
                      WHERE n.entrada_id = e.id AND n.tipo = 'nota'
                      ORDER BY n.id DESC LIMIT 1)
               FROM entrada e LEFT JOIN recurso r ON r.clave = e.recurso
              WHERE e.id = ?1",
            [id],
            |fila| {
                Ok(FilaEntrada {
                    id: fila.get(0)?,
                    dia: fila.get(1)?,
                    creada: fila.get(2)?,
                    tipo: fila.get(3)?,
                    origen: fila.get(4)?,
                    fuente: fila.get(5)?,
                    url: fila.get(6)?,
                    titulo: fila.get(7)?,
                    clave: fila.get(8)?,
                    fijada: fila.get(9)?,
                    capturas: fila.get(10)?,
                    borrada: fila.get(11)?,
                    seleccion: fila.get(12)?,
                    estado: fila.get(13)?,
                    proyecto: fila.get(14)?,
                    app: fila.get(15)?,
                    cambios: fila.get(16)?,
                    notas: fila.get(17)?,
                    ultima_nota: fila.get(18)?,
                })
            },
        )
        .optional()?;
    let Some(fila) = fila else {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    };

    let app = app_de(conn, fila.app.as_deref(), fila.clave.as_deref(), fila.id)?;
    let proyecto = match fila.proyecto {
        Some(proyecto) if !proyecto.is_empty() => Some(proyecto),
        _ => match entidad_url(conn, fila.id, "proyecto:")? {
            Some(proyecto) => Some(proyecto),
            None => entidad_prefijada(conn, fila.id, "proyecto:").ok().flatten(),
        },
    };
    let host = if fila.fuente == "web" {
        fila.url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
            .and_then(|url| url.host_str().map(str::to_string))
    } else {
        None
    };
    Ok(EntradaVista {
        id: fila.id,
        dia: fila.dia,
        hora: hora_de(&fila.creada),
        creada: fila.creada,
        tipo: tipo_entrada_de_str(&fila.tipo),
        origen: origen_de_str(&fila.origen),
        fuente: fuente_de_str(&fila.fuente),
        url: fila.url,
        titulo: fila.titulo,
        clave: fila.clave.as_deref().map(recurso::visible),
        estado: fila.estado,
        app,
        proyecto,
        host,
        seleccion: fila.seleccion.as_deref().map(|texto| recortar(texto, 280)),
        notas: u32::try_from(fila.notas).unwrap_or(0),
        ultima_nota: fila
            .ultima_nota
            .as_deref()
            .map(|texto| recortar(texto, 140)),
        cambios: fila
            .cambios
            .as_deref()
            .map(leer_cambios)
            .unwrap_or_default(),
        fijada: fila.fijada != 0,
        relacionadas: 0,
        capturas: u32::try_from(fila.capturas).unwrap_or(0),
        fragmento: None,
        borrada: fila.borrada,
    })
}

/// Detalle completo de una entrada.
pub fn detalle(conn: &Connection, id: i64) -> Resultado<DetalleEntrada> {
    let mut entrada = entrada_vista_base(conn, id)?;
    entrada.relacionadas = relaciones::contar(conn, id)?;
    let seleccion: Option<String> = conn.query_row(
        "SELECT seleccion FROM entrada WHERE id = ?1",
        [id],
        |fila| fila.get(0),
    )?;
    let notas = notas_de(conn, id)?;
    let relacionadas = relaciones::relacionadas(conn, id, 12)?;
    let historial = historial_de(conn, id)?;
    let clave: Option<String> =
        conn.query_row("SELECT recurso FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })?;
    let recurso = match clave {
        Some(clave) => recurso_vista(conn, &clave)?,
        None => None,
    };
    Ok(DetalleEntrada {
        entrada,
        seleccion,
        notas,
        relacionadas,
        historial,
        recurso,
    })
}

/// Crea una nota (`tipo = nota`) y recalcula FTS y entidades de la entrada.
pub fn crear_nota(
    conn: &Connection,
    ajustes: &Ajustes,
    entrada_id: i64,
    texto: &str,
    ahora: DateTime<Local>,
) -> Resultado<Nota> {
    let texto = validar_texto_nota(texto)?;
    exigir_entrada(conn, entrada_id)?;
    let creada = ahora.to_rfc3339();
    let ctx = entidades::Contexto::cargar(conn, ajustes)?;
    let tx = db::transaccion_inmediata(conn)?;
    tx.execute(
        "INSERT INTO nota (entrada_id, creada, tipo, texto) VALUES (?1, ?2, 'nota', ?3)",
        params![entrada_id, creada, texto],
    )?;
    let id = tx.last_insert_rowid();
    entidades::recalcular(&tx, entrada_id, &ctx)?;
    captura::reindexar(&tx, entrada_id)?;
    let nota = nota_por_id(&tx, id)?;
    tx.commit()?;
    Ok(nota)
}

/// Edita el texto de una nota y recalcula FTS y entidades.
pub fn editar_nota(
    conn: &Connection,
    ajustes: &Ajustes,
    id: i64,
    texto: &str,
    ahora: DateTime<Local>,
) -> Resultado<Nota> {
    let texto = validar_texto_nota(texto)?;
    let entrada_id = entrada_de_nota(conn, id)?;
    let ctx = entidades::Contexto::cargar(conn, ajustes)?;
    let tx = db::transaccion_inmediata(conn)?;
    tx.execute(
        "UPDATE nota SET texto = ?2, editada = ?3 WHERE id = ?1",
        params![id, texto, ahora.to_rfc3339()],
    )?;
    entidades::recalcular(&tx, entrada_id, &ctx)?;
    captura::reindexar(&tx, entrada_id)?;
    let nota = nota_por_id(&tx, id)?;
    tx.commit()?;
    Ok(nota)
}

/// Borra una nota y recalcula FTS y entidades.
pub fn borrar_nota(conn: &Connection, ajustes: &Ajustes, id: i64) -> Resultado<()> {
    let entrada_id = entrada_de_nota(conn, id)?;
    let ctx = entidades::Contexto::cargar(conn, ajustes)?;
    let tx = db::transaccion_inmediata(conn)?;
    tx.execute("DELETE FROM nota WHERE id = ?1", [id])?;
    entidades::recalcular(&tx, entrada_id, &ctx)?;
    captura::reindexar(&tx, entrada_id)?;
    tx.commit()?;
    Ok(())
}

/// Cambia el título de una entrada.
pub fn editar_titulo(
    conn: &Connection,
    ajustes: &Ajustes,
    id: i64,
    titulo: &str,
    ahora: DateTime<Local>,
) -> Resultado<()> {
    let titulo = titulo.trim();
    if titulo.is_empty() {
        return Err(Error::Validacion("El título no puede estar vacío".into()));
    }
    if titulo.chars().count() > 1000 {
        return Err(Error::Validacion("El título es demasiado largo".into()));
    }
    exigir_entrada(conn, id)?;
    let ctx = entidades::Contexto::cargar(conn, ajustes)?;
    let tx = db::transaccion_inmediata(conn)?;
    tx.execute(
        "UPDATE entrada SET titulo = ?2, actualizada = ?3 WHERE id = ?1",
        params![id, titulo, ahora.to_rfc3339()],
    )?;
    entidades::recalcular(&tx, id, &ctx)?;
    captura::reindexar(&tx, id)?;
    tx.commit()?;
    Ok(())
}

/// Fija o desfija una entrada.
pub fn fijar(conn: &Connection, id: i64, fijada: bool) -> Resultado<()> {
    let tx = db::transaccion_inmediata(conn)?;
    exigir_entrada(&tx, id)?;
    tx.execute(
        "UPDATE entrada SET fijada = ?2 WHERE id = ?1",
        params![id, i64::from(fijada)],
    )?;
    tx.commit()?;
    Ok(())
}

/// A la papelera (recuperable 30 días).
pub fn borrar(conn: &Connection, id: i64, ahora: DateTime<Local>) -> Resultado<()> {
    let tx = db::transaccion_inmediata(conn)?;
    exigir_entrada(&tx, id)?;
    tx.execute(
        "UPDATE entrada SET borrada = ?2 WHERE id = ?1",
        params![id, ahora.to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Borra para siempre una entrada que ya está en la papelera: su fila de FTS, sus tablas
/// hijas (por cascada) y la entrada. Fuera de la papelera no se puede.
///
/// `sello` es el `borrada` que vio la UI al pedir la confirmación: SQLite puede reutilizar
/// un id tras un borrado definitivo, y sin el sello una confirmación vieja borraría otra miga.
pub fn borrar_definitivo(conn: &Connection, id: i64, sello: &str) -> Resultado<()> {
    let tx = db::transaccion_inmediata(conn)?;
    let borrada: Option<Option<String>> = tx
        .query_row("SELECT borrada FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })
        .optional()?;
    let Some(borrada) = borrada else {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    };
    match borrada {
        None => {
            return Err(Error::Validacion(
                "Solo se puede borrar definitivamente desde la papelera".into(),
            ));
        }
        Some(borrada) if borrada != sello => {
            return Err(Error::Validacion(
                "Esa miga ha cambiado; vuelve a intentarlo".into(),
            ));
        }
        Some(_) => {}
    }
    db::borrar_entradas(&tx, &[id])?;
    tx.commit()?;
    Ok(())
}

/// Vacía la papelera: borra de verdad todas las entradas con `borrada IS NOT NULL`, sin
/// mirar la antigüedad. Nunca toca las vivas. Devuelve cuántas borró.
pub fn vaciar_papelera(conn: &Connection) -> Resultado<usize> {
    let tx = db::transaccion_inmediata(conn)?;
    let ids: Vec<i64> = {
        let mut consulta = tx.prepare("SELECT id FROM entrada WHERE borrada IS NOT NULL")?;
        let filas = consulta.query_map([], |fila| fila.get(0))?;
        filas.collect::<Result<Vec<i64>, _>>()?
    };
    let borradas = db::borrar_entradas(&tx, &ids)?;
    tx.commit()?;
    Ok(borradas)
}

/// Restaura de la papelera. Si choca con el índice único de día+URL, no toca nada.
pub fn restaurar(conn: &Connection, id: i64) -> Resultado<()> {
    let tx = db::transaccion_inmediata(conn)?;
    let fila: Option<(String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT dia, url_norm, borrada FROM entrada WHERE id = ?1",
            [id],
            |fila| Ok((fila.get(0)?, fila.get(1)?, fila.get(2)?)),
        )
        .optional()?;
    let Some((dia, url_norm, borrada)) = fila else {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    };
    if borrada.is_none() {
        return Ok(());
    }
    if let Some(url_norm) = url_norm {
        let choques: i64 = tx.query_row(
            "SELECT COUNT(*) FROM entrada
              WHERE dia = ?1 AND url_norm = ?2 AND borrada IS NULL AND id <> ?3",
            params![dia, url_norm, id],
            |fila| fila.get(0),
        )?;
        if choques > 0 {
            return Err(Error::Validacion("Ya hay otra igual ese día".into()));
        }
    }
    tx.execute("UPDATE entrada SET borrada = NULL WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

/// Enlace manual entre dos entradas. El par se normaliza con `a < b`.
pub fn enlazar(conn: &Connection, a: i64, b: i64, ahora: DateTime<Local>) -> Resultado<()> {
    let (a, b) = normalizar_par(a, b)?;
    let tx = db::transaccion_inmediata(conn)?;
    exigir_entrada(&tx, a)?;
    exigir_entrada(&tx, b)?;
    tx.execute(
        "INSERT INTO enlace_manual (a, b, creada) VALUES (?1, ?2, ?3)
         ON CONFLICT(a, b) DO UPDATE SET creada = excluded.creada",
        params![a, b, ahora.to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Quita un enlace manual.
pub fn desenlazar(conn: &Connection, a: i64, b: i64) -> Resultado<()> {
    let (a, b) = normalizar_par(a, b)?;
    let tx = db::transaccion_inmediata(conn)?;
    tx.execute(
        "DELETE FROM enlace_manual WHERE a = ?1 AND b = ?2",
        params![a, b],
    )?;
    tx.commit()?;
    Ok(())
}

/// Descarta una relación automática.
pub fn descartar(conn: &Connection, a: i64, b: i64) -> Resultado<()> {
    let (a, b) = normalizar_par(a, b)?;
    let tx = db::transaccion_inmediata(conn)?;
    exigir_entrada(&tx, a)?;
    exigir_entrada(&tx, b)?;
    tx.execute(
        "INSERT OR IGNORE INTO descarte (a, b) VALUES (?1, ?2)",
        params![a, b],
    )?;
    tx.commit()?;
    Ok(())
}

/// Deja de seguir (o vuelve a seguir) el recurso de una entrada.
pub fn seguir(conn: &Connection, id: i64, seguir: bool) -> Resultado<()> {
    let tx = db::transaccion_inmediata(conn)?;
    let clave: Option<Option<String>> = tx
        .query_row("SELECT recurso FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })
        .optional()?;
    let Some(clave) = clave else {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    };
    let Some(clave) = clave else {
        return Err(Error::Validacion("La entrada no tiene recurso".into()));
    };
    tx.execute(
        "UPDATE recurso SET seguir = ?2 WHERE clave = ?1",
        params![clave, i64::from(seguir)],
    )?;
    tx.commit()?;
    Ok(())
}

/// Marca visto el recurso de una entrada (no hace nada si no tiene).
pub fn marcar_visto_entrada(conn: &Connection, id: i64, ahora: DateTime<Local>) -> Resultado<()> {
    let tx = db::transaccion_inmediata(conn)?;
    let clave: Option<Option<String>> = tx
        .query_row("SELECT recurso FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })
        .optional()?;
    let Some(Some(clave)) = clave else {
        exigir_entrada(&tx, id)?;
        tx.commit()?;
        return Ok(());
    };
    recurso::marcar_visto(&tx, &clave, ahora)?;
    tx.commit()?;
    Ok(())
}

/// Marca vistos todos los recursos con cambios. Devuelve cuántos.
pub fn marcar_todo_visto(conn: &Connection, ahora: DateTime<Local>) -> Resultado<u32> {
    let tx = db::transaccion_inmediata(conn)?;
    let marcados = tx.execute(
        "UPDATE recurso
            SET huella_vista = huella_actual,
                cambios = NULL,
                visto = ?1,
                pendiente_visto = CASE WHEN huella_actual IS NULL THEN 1 ELSE 0 END
          WHERE cambios IS NOT NULL",
        [ahora.to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(u32::try_from(marcados).unwrap_or(u32::MAX))
}

/// Entradas (recursos distintos) con cambios sin ver.
pub fn contar_cambios(conn: &Connection) -> Resultado<u32> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM recurso r
          WHERE r.cambios IS NOT NULL
            AND EXISTS (SELECT 1 FROM entrada e WHERE e.recurso = r.clave AND e.borrada IS NULL)",
        [],
        |fila| fila.get(0),
    )?;
    Ok(u32::try_from(total).unwrap_or(u32::MAX))
}

/// Estado de una URL para el mensaje `estado` del host.
pub fn estado_url(
    conn: &Connection,
    url: &str,
    ajustes: &Ajustes,
    ahora: DateTime<Local>,
) -> Resultado<EstadoUrl> {
    let reconocido = recurso::reconocer(url, ajustes);
    let (clave, url_norm) = match &reconocido {
        Some(reconocido) => (Some(reconocido.clave.clone()), reconocido.url.clone()),
        None => (None, recurso::normalizar(url)),
    };
    let hoy = hoy(ahora);
    let fila: Option<(Option<i64>, i64, i64)> = match &clave {
        Some(clave) => conn
            .query_row(
                "SELECT MAX(id), COUNT(DISTINCT dia),
                        COALESCE(SUM(CASE WHEN dia = ?2 THEN 1 ELSE 0 END), 0)
                   FROM entrada WHERE recurso = ?1 AND borrada IS NULL",
                params![clave, hoy],
                |fila| Ok((fila.get(0)?, fila.get(1)?, fila.get(2)?)),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT MAX(id), COUNT(DISTINCT dia),
                        COALESCE(SUM(CASE WHEN dia = ?2 THEN 1 ELSE 0 END), 0)
                   FROM entrada WHERE url_norm = ?1 AND borrada IS NULL",
                params![url_norm, hoy],
                |fila| Ok((fila.get(0)?, fila.get(1)?, fila.get(2)?)),
            )
            .optional()?,
    };
    let (id, veces, guardadas_hoy) = fila.unwrap_or((None, 0, 0));
    let cambios = match &clave {
        Some(clave) => contar_cambios_de(conn, clave)?,
        None => 0,
    };
    let relacionadas = match id {
        Some(id) => relaciones::contar(conn, id)?,
        None => 0,
    };
    Ok(EstadoUrl {
        guardada_hoy: guardadas_hoy > 0,
        id,
        veces: u32::try_from(veces).unwrap_or(0),
        cambios,
        relacionadas,
    })
}

// ───────────────────────── Ayudas internas ─────────────────────────

fn hoy(ahora: DateTime<Local>) -> String {
    ahora.format("%Y-%m-%d").to_string()
}

fn recortar(texto: &str, maximo: usize) -> String {
    texto.chars().take(maximo).collect()
}

fn hora_de(creada: &str) -> String {
    util::parse_fecha(creada)
        .map(|fecha| fecha.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

fn leer_cambios(texto: &str) -> Vec<Cambio> {
    serde_json::from_str(texto).unwrap_or_default()
}

fn validar_texto_nota(texto: &str) -> Resultado<String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Err(Error::Validacion("La nota está vacía".into()));
    }
    Ok(recortar(texto, 20_000))
}

fn exigir_entrada(conn: &Connection, id: i64) -> Resultado<()> {
    let existe: Option<i64> = conn
        .query_row("SELECT id FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })
        .optional()?;
    match existe {
        Some(_) => Ok(()),
        None => Err(Error::NoExiste(format!("la entrada {id}"))),
    }
}

fn normalizar_par(a: i64, b: i64) -> Resultado<(i64, i64)> {
    if a == b {
        return Err(Error::Validacion(
            "No se puede relacionar una entrada consigo misma".into(),
        ));
    }
    Ok((a.min(b), a.max(b)))
}

fn entrada_de_nota(conn: &Connection, id: i64) -> Resultado<i64> {
    conn.query_row("SELECT entrada_id FROM nota WHERE id = ?1", [id], |fila| {
        fila.get(0)
    })
    .optional()?
    .ok_or_else(|| Error::NoExiste(format!("la nota {id}")))
}

fn nota_por_id(conn: &Connection, id: i64) -> Resultado<Nota> {
    conn.query_row(
        "SELECT id, entrada_id, creada, editada, tipo, texto FROM nota WHERE id = ?1",
        [id],
        |fila| {
            Ok(Nota {
                id: fila.get(0)?,
                entrada_id: fila.get(1)?,
                creada: fila.get(2)?,
                editada: fila.get(3)?,
                tipo: fila.get(4)?,
                texto: fila.get(5)?,
            })
        },
    )
    .map_err(Into::into)
}

fn notas_de(conn: &Connection, entrada_id: i64) -> Resultado<Vec<Nota>> {
    let mut consulta = conn.prepare(
        "SELECT id, entrada_id, creada, editada, tipo, texto
           FROM nota WHERE entrada_id = ?1 ORDER BY creada, id",
    )?;
    let filas = consulta.query_map([entrada_id], |fila| {
        Ok(Nota {
            id: fila.get(0)?,
            entrada_id: fila.get(1)?,
            creada: fila.get(2)?,
            editada: fila.get(3)?,
            tipo: fila.get(4)?,
            texto: fila.get(5)?,
        })
    })?;
    Ok(filas.collect::<Result<Vec<Nota>, _>>()?)
}

fn historial_de(conn: &Connection, id: i64) -> Resultado<Vec<Aparicion>> {
    let mut consulta = conn.prepare(
        "SELECT id, dia, creada FROM entrada
          WHERE borrada IS NULL AND id <> ?1
            AND ((url_norm IS NOT NULL
                  AND url_norm = (SELECT url_norm FROM entrada WHERE id = ?1))
                 OR (recurso IS NOT NULL
                     AND recurso = (SELECT recurso FROM entrada WHERE id = ?1)))
          ORDER BY dia DESC, id DESC",
    )?;
    let filas = consulta.query_map([id], |fila| {
        let creada: String = fila.get(2)?;
        Ok(Aparicion {
            id: fila.get(0)?,
            dia: fila.get(1)?,
            hora: hora_de(&creada),
        })
    })?;
    Ok(filas.collect::<Result<Vec<Aparicion>, _>>()?)
}

/// Fila de `recurso` con sus huellas en JSON.
struct FilaRecurso {
    clave: String,
    tipo: String,
    url: String,
    titulo: Option<String>,
    estado: Option<String>,
    huella_actual: Option<String>,
    huella_vista: Option<String>,
    comprobado: Option<String>,
    error: Option<String>,
    visto: Option<String>,
    seguir: i64,
    cambios: Option<String>,
}

fn recurso_vista(conn: &Connection, clave: &str) -> Resultado<Option<RecursoVista>> {
    let fila: Option<FilaRecurso> = conn
        .query_row(
            "SELECT clave, tipo, url, titulo, estado, huella_actual, huella_vista,
                    comprobado, error, visto, seguir, cambios
               FROM recurso WHERE clave = ?1",
            [clave],
            |fila| {
                Ok(FilaRecurso {
                    clave: fila.get(0)?,
                    tipo: fila.get(1)?,
                    url: fila.get(2)?,
                    titulo: fila.get(3)?,
                    estado: fila.get(4)?,
                    huella_actual: fila.get(5)?,
                    huella_vista: fila.get(6)?,
                    comprobado: fila.get(7)?,
                    error: fila.get(8)?,
                    visto: fila.get(9)?,
                    seguir: fila.get(10)?,
                    cambios: fila.get(11)?,
                })
            },
        )
        .optional()?;
    let Some(fila) = fila else {
        return Ok(None);
    };
    let actual: Huella = fila
        .huella_actual
        .as_deref()
        .and_then(|texto| serde_json::from_str(texto).ok())
        .unwrap_or_default();
    let vista: Huella = fila
        .huella_vista
        .as_deref()
        .and_then(|texto| serde_json::from_str(texto).ok())
        .unwrap_or_default();
    Ok(Some(RecursoVista {
        clave: fila.clave,
        tipo: tipo_recurso_de_str(&fila.tipo),
        url: fila.url,
        titulo: fila.titulo,
        estado: fila.estado,
        asignado: actual.asignado.clone(),
        comprobado: fila.comprobado,
        error: fila.error,
        visto: fila.visto,
        seguir: fila.seguir != 0,
        cambios: fila
            .cambios
            .as_deref()
            .map(leer_cambios)
            .unwrap_or_default(),
        ultimos: actual.ultimos.clone(),
        nuevos: huella::nuevos(&vista, &actual),
    }))
}

fn contar_cambios_de(conn: &Connection, clave: &str) -> Resultado<u32> {
    let json: Option<String> = conn
        .query_row(
            "SELECT cambios FROM recurso WHERE clave = ?1",
            [clave],
            |fila| fila.get(0),
        )
        .optional()?
        .flatten();
    Ok(json
        .as_deref()
        .map(leer_cambios)
        .map_or(0, |cambios| u32::try_from(cambios.len()).unwrap_or(0)))
}

/// App preferente: URL > recurso remoto > texto.
fn app_de(
    conn: &Connection,
    app_recurso: Option<&str>,
    clave: Option<&str>,
    entrada_id: i64,
) -> Resultado<Option<String>> {
    // URL > recurso remoto > texto. La app de la URL ya está validada por host en `entidad`.
    if let Some(app) = entidad_url(conn, entrada_id, "app:")? {
        return Ok(Some(app));
    }
    if let Some(app) = app_recurso.filter(|app| !app.is_empty()) {
        return Ok(Some(app.to_string()));
    }
    if let Some(clave) = clave
        && let Some(app) = entidad_prefijada_de(conn, "recurso_entidad", "clave", clave, "app:")?
    {
        return Ok(Some(app));
    }
    entidad_prefijada(conn, entrada_id, "app:")
}

/// Primera entidad de la entrada con ese prefijo y `fuente = 'url'`.
fn entidad_url(conn: &Connection, entrada_id: i64, prefijo: &str) -> Resultado<Option<String>> {
    let como = format!("{prefijo}%");
    let encontrado: Option<String> = conn
        .query_row(
            "SELECT valor FROM entidad
              WHERE entrada_id = ?1 AND valor LIKE ?2 AND fuente = 'url'
              ORDER BY valor LIMIT 1",
            params![entrada_id, como],
            |fila| fila.get(0),
        )
        .optional()?;
    Ok(encontrado.and_then(|entidad| {
        entidad
            .strip_prefix(prefijo)
            .map(str::to_string)
            .filter(|valor| !valor.is_empty())
    }))
}

fn entidad_prefijada(
    conn: &Connection,
    entrada_id: i64,
    prefijo: &str,
) -> Resultado<Option<String>> {
    entidad_prefijada_de(
        conn,
        "entidad",
        "entrada_id",
        &entrada_id.to_string(),
        prefijo,
    )
}

fn entidad_prefijada_de(
    conn: &Connection,
    tabla: &str,
    columna: &str,
    valor: &str,
    prefijo: &str,
) -> Resultado<Option<String>> {
    let sql = format!(
        "SELECT valor FROM {tabla} WHERE {columna} = ?1 AND valor LIKE ?2 ORDER BY valor LIMIT 1"
    );
    let como = format!("{prefijo}%");
    let encontrado: Option<String> = conn
        .query_row(&sql, params![valor, como], |fila| fila.get(0))
        .optional()?;
    Ok(encontrado.and_then(|entidad| {
        entidad
            .strip_prefix(prefijo)
            .map(str::to_string)
            .filter(|app| !app.is_empty())
    }))
}

fn consulta_fts(texto: &str) -> Option<String> {
    let palabras: Vec<String> = texto
        .split_whitespace()
        .map(|palabra| palabra.replace('"', ""))
        .filter(|palabra| !palabra.is_empty())
        .map(|palabra| format!("\"{palabra}\"*"))
        .collect();
    if palabras.is_empty() {
        None
    } else {
        Some(palabras.join(" "))
    }
}

fn fragmentos(conn: &Connection, consulta: &str) -> Resultado<HashMap<i64, Vec<Trozo>>> {
    let mut consulta_sql = conn.prepare(
        "SELECT rowid, snippet(entrada_fts, -1, char(1), char(2), '…', 14)
           FROM entrada_fts WHERE entrada_fts MATCH ?1",
    )?;
    let filas = consulta_sql.query_map([consulta], |fila| {
        Ok((fila.get::<_, i64>(0)?, fila.get::<_, String>(1)?))
    })?;
    let mut salida = HashMap::new();
    for fila in filas {
        let (id, texto) = fila?;
        salida.insert(id, trocear(&texto));
    }
    Ok(salida)
}

/// Convierte el `snippet` de FTS5 en trozos: `char(1)` abre resaltado y `char(2)` lo cierra.
fn trocear(texto: &str) -> Vec<Trozo> {
    let mut trozos: Vec<Trozo> = Vec::new();
    let mut actual = String::new();
    let mut marcado = false;
    for caracter in texto.chars() {
        match caracter {
            '\u{1}' => {
                empujar(&mut trozos, &mut actual, false);
                marcado = true;
            }
            '\u{2}' => {
                empujar(&mut trozos, &mut actual, true);
                marcado = false;
            }
            _ => actual.push(caracter),
        }
    }
    empujar(&mut trozos, &mut actual, marcado);
    trozos
}

fn empujar(trozos: &mut Vec<Trozo>, actual: &mut String, marcado: bool) {
    if actual.is_empty() {
        return;
    }
    if let Some(ultimo) = trozos.last_mut()
        && ultimo.m == marcado
    {
        ultimo.t.push_str(actual);
        actual.clear();
        return;
    }
    trozos.push(Trozo {
        t: std::mem::take(actual),
        m: marcado,
    });
}

fn fuente_como_str(fuente: Fuente) -> &'static str {
    match fuente {
        Fuente::Jira => "jira",
        Fuente::Gitlab => "gitlab",
        Fuente::Fs => "fs",
        Fuente::Web => "web",
        Fuente::Texto => "texto",
        Fuente::Nota => "nota",
    }
}

fn fuente_de_str(fuente: &str) -> Fuente {
    match fuente {
        "jira" => Fuente::Jira,
        "gitlab" => Fuente::Gitlab,
        "fs" => Fuente::Fs,
        "web" => Fuente::Web,
        "nota" => Fuente::Nota,
        _ => Fuente::Texto,
    }
}

fn tipo_entrada_de_str(tipo: &str) -> TipoEntrada {
    match tipo {
        "enlace" => TipoEntrada::Enlace,
        "nota" => TipoEntrada::Nota,
        _ => TipoEntrada::Texto,
    }
}

fn origen_de_str(origen: &str) -> Origen {
    match origen {
        "navegador" => Origen::Navegador,
        "global" => Origen::Global,
        "app" => Origen::App,
        _ => Origen::Cli,
    }
}

fn tipo_recurso_de_str(tipo: &str) -> TipoRecurso {
    match tipo {
        "mr" => TipoRecurso::Mr,
        "issue" => TipoRecurso::Issue,
        "fs" => TipoRecurso::Fs,
        _ => TipoRecurso::Jira,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captura::capturar;
    use crate::db;
    use crate::model::{Comentario, NuevaEntrada};
    use chrono::TimeZone;

    fn bd(dir: &std::path::Path) -> Connection {
        db::abrir(&dir.join("migas.db")).expect("BD de prueba")
    }

    fn sello(conn: &Connection, id: i64) -> String {
        conn.query_row("SELECT borrada FROM entrada WHERE id = ?1", [id], |f| {
            f.get(0)
        })
        .expect("sello de papelera")
    }

    fn recursos_huerfanos(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM recurso r
              WHERE NOT EXISTS (SELECT 1 FROM entrada e WHERE e.recurso = r.clave)",
            [],
            |f| f.get(0),
        )
        .expect("huérfanos")
    }

    #[test]
    fn borrar_definitivo_con_sello_viejo_no_borra_otra_miga() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let a = texto(&conn, "miga A", 4, 10);
        borrar(&conn, a, cuando(4, 11)).expect("A a la papelera");
        let sello_a = sello(&conn, a);
        vaciar_papelera(&conn).expect("vacía");
        // SQLite reutiliza el id de A para B.
        let b = texto(&conn, "miga B", 4, 12);
        assert_eq!(a, b, "el escenario necesita reutilizar el id");
        borrar(&conn, b, cuando(4, 13)).expect("B a la papelera");
        let error = borrar_definitivo(&conn, b, &sello_a).expect_err("sello viejo");
        assert_eq!(
            error.to_string(),
            "Esa miga ha cambiado; vuelve a intentarlo"
        );
        let quedan: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada WHERE id = ?1", [b], |f| {
                f.get(0)
            })
            .expect("B sigue");
        assert_eq!(quedan, 1);
    }

    #[test]
    fn borrado_definitivo_no_deja_recursos_huerfanos() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let url = "https://empresa.atlassian.net/browse/PROJ-77";
        let suelta = enlace(&conn, url, 4, 10);
        let compartida_vieja = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-78", 4, 10);
        let compartida_viva = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-78", 5, 10);
        let antes: i64 = conn
            .query_row("SELECT COUNT(*) FROM recurso", [], |f| f.get(0))
            .expect("recursos");
        assert_eq!(antes, 2);

        borrar(&conn, suelta, cuando(4, 11)).expect("papelera");
        borrar_definitivo(&conn, suelta, &sello(&conn, suelta)).expect("borra");
        assert_eq!(recursos_huerfanos(&conn), 0);

        borrar(&conn, compartida_vieja, cuando(4, 12)).expect("papelera");
        vaciar_papelera(&conn).expect("vacía");
        assert_eq!(recursos_huerfanos(&conn), 0);
        let queda: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM recurso r JOIN entrada e ON e.recurso = r.clave WHERE e.id = ?1",
                [compartida_viva],
                |f| f.get(0),
            )
            .expect("recurso compartido");
        assert_eq!(queda, 1, "el recurso de la miga viva se conserva");
    }

    fn cuando(dia: u32, hora: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, dia, hora, 0, 0).unwrap()
    }

    fn enlace(conn: &Connection, url: &str, dia: u32, hora: u32) -> i64 {
        capturar(
            conn,
            &NuevaEntrada {
                tipo: TipoEntrada::Enlace,
                origen: Origen::Navegador,
                url: Some(url.into()),
                titulo: None,
                seleccion: None,
                nota: None,
            },
            &Ajustes::default(),
            cuando(dia, hora),
        )
        .expect("captura enlace")
        .id
    }

    fn texto(conn: &Connection, texto: &str, dia: u32, hora: u32) -> i64 {
        capturar(
            conn,
            &NuevaEntrada {
                tipo: TipoEntrada::Texto,
                origen: Origen::Global,
                url: None,
                titulo: None,
                seleccion: Some(texto.into()),
                nota: None,
            },
            &Ajustes::default(),
            cuando(dia, hora),
        )
        .expect("captura texto")
        .id
    }

    fn filtro(vista: Vista) -> Filtro {
        Filtro {
            vista: Some(vista),
            ..Filtro::default()
        }
    }

    #[test]
    fn listar_dias_con_rango_y_cambios() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 3, 10);
        enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        texto(&conn, "suelta", 4, 11);

        let dias = listar_dias(&conn, None, None).expect("días");
        assert_eq!(dias.len(), 2);
        assert_eq!(dias[0].dia, "2026-11-04");
        assert_eq!(dias[0].total, 2);
        assert_eq!(dias[1].dia, "2026-11-03");

        let rango = listar_dias(&conn, Some("2026-11-04"), Some("2026-11-04")).expect("rango");
        assert_eq!(rango.len(), 1);
        assert_eq!(rango[0].total, 2);

        conn.execute(
            "UPDATE recurso SET cambios = '[{\"tipo\":\"estado\"}]' WHERE clave = 'jira:PROJ-1'",
            [],
        )
        .expect("cambios");
        let dias = listar_dias(&conn, None, None).expect("días con cambios");
        assert_eq!(dias[0].cambios, 1);
        assert_eq!(dias[1].cambios, 1);
    }

    #[test]
    fn listar_cada_vista_y_fuentes() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let jira = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 9);
        let web = enlace(&conn, "https://ejemplo.com/a", 4, 10);
        let texto_id = texto(&conn, "una nota", 4, 11);
        let hoy = cuando(4, 12);

        let dia = listar(&conn, &filtro(Vista::Dia), hoy).expect("día");
        assert_eq!(dia.total, 3);
        assert_eq!(dia.entradas[0].id, texto_id, "más reciente primero");
        assert_eq!(dia.entradas[0].hora, "11:00");

        let solo_web = listar(
            &conn,
            &Filtro {
                vista: Some(Vista::Todo),
                fuentes: vec![Fuente::Web],
                ..Filtro::default()
            },
            hoy,
        )
        .expect("fuentes");
        assert_eq!(solo_web.total, 1);
        assert_eq!(solo_web.entradas[0].id, web);
        assert_eq!(solo_web.entradas[0].host.as_deref(), Some("ejemplo.com"));

        fijar(&conn, jira, true).expect("fija");
        let fijadas = listar(&conn, &filtro(Vista::Fijadas), hoy).expect("fijadas");
        assert_eq!(fijadas.total, 1);
        assert_eq!(fijadas.entradas[0].id, jira);

        borrar(&conn, texto_id, cuando(4, 12)).expect("borra");
        let papelera = listar(&conn, &filtro(Vista::Papelera), hoy).expect("papelera");
        assert_eq!(papelera.total, 1);
        assert_eq!(papelera.entradas[0].id, texto_id);
        assert!(papelera.entradas[0].borrada.is_some());
        let dia = listar(&conn, &filtro(Vista::Dia), hoy).expect("día otra vez");
        assert_eq!(dia.total, 2);
    }

    #[test]
    fn listar_rellena_el_contador_de_relacionadas() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let vieja = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 3, 10);
        let nueva = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        let listado = listar(&conn, &filtro(Vista::Todo), cuando(4, 12)).expect("listado");
        let entrada = listado
            .entradas
            .iter()
            .find(|entrada| entrada.id == nueva)
            .expect("entrada nueva");
        assert_eq!(entrada.relacionadas, 1);
        let entrada = listado
            .entradas
            .iter()
            .find(|entrada| entrada.id == vieja)
            .expect("entrada vieja");
        assert_eq!(entrada.relacionadas, 1);
    }

    #[test]
    fn vista_cambios_una_por_recurso() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let vieja = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 3, 10);
        let nueva = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        let sin = enlace(&conn, "https://ejemplo.com/a", 4, 11);
        conn.execute(
            "UPDATE recurso SET cambios = '[{\"tipo\":\"estado\"}]' WHERE clave = 'jira:PROJ-1'",
            [],
        )
        .expect("cambios");

        let cambios = listar(&conn, &filtro(Vista::Cambios), cuando(4, 12)).expect("cambios");
        assert_eq!(cambios.total, 1);
        assert_eq!(cambios.entradas[0].id, nueva);
        assert_ne!(cambios.entradas[0].id, vieja);
        assert_ne!(cambios.entradas[0].id, sin);
        assert_eq!(contar_cambios(&conn).expect("cuenta"), 1);
    }

    #[test]
    fn busqueda_fts_con_acentos_fragmento_y_limite() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let id = enlace(
            &conn,
            "https://empresa.atlassian.net/browse/PROJ-1896",
            4,
            10,
        );
        editar_titulo(
            &conn,
            &Ajustes::default(),
            id,
            "Listado de Gestión documental",
            cuando(4, 11),
        )
        .expect("título");
        crear_nota(
            &conn,
            &Ajustes::default(),
            id,
            "pendiente con Ana",
            cuando(4, 12),
        )
        .expect("nota");
        texto(&conn, "otra miga distinta", 4, 13);

        let sin_acento = listar(
            &conn,
            &Filtro {
                vista: Some(Vista::Todo),
                texto: Some("gestion".into()),
                ..Filtro::default()
            },
            cuando(4, 14),
        )
        .expect("busca");
        assert_eq!(sin_acento.total, 1);
        assert_eq!(sin_acento.entradas[0].id, id);
        let fragmento = sin_acento.entradas[0]
            .fragmento
            .as_ref()
            .expect("fragmento");
        assert!(
            fragmento.iter().any(|trozo| trozo.m),
            "debe haber resaltado: {fragmento:?}"
        );
        assert!(
            fragmento
                .iter()
                .any(|trozo| trozo.t.to_lowercase().contains("gestión"))
        );

        // Dos palabras y comillas.
        let dos = listar(
            &conn,
            &Filtro {
                vista: Some(Vista::Todo),
                texto: Some("\"listado\" ana".into()),
                ..Filtro::default()
            },
            cuando(4, 14),
        )
        .expect("dos palabras");
        assert_eq!(dos.total, 1);

        // El extra del recurso también se busca (clave visible).
        let clave = listar(
            &conn,
            &Filtro {
                vista: Some(Vista::Todo),
                texto: Some("PROJ-1896".into()),
                ..Filtro::default()
            },
            cuando(4, 14),
        )
        .expect("clave");
        assert_eq!(clave.total, 1);

        let limitado = listar(
            &conn,
            &Filtro {
                vista: Some(Vista::Todo),
                limite: Some(1),
                ..Filtro::default()
            },
            cuando(4, 14),
        )
        .expect("límite");
        assert_eq!(limitado.entradas.len(), 1);
        assert_eq!(limitado.total, 2, "total sin límite");
    }

    #[test]
    fn detalle_con_recurso_notas_e_historial() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let vieja = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 3, 10);
        let nueva = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        let seleccion = "s".repeat(300);
        conn.execute(
            "UPDATE entrada SET seleccion = ?2 WHERE id = ?1",
            params![nueva, seleccion],
        )
        .expect("selección");
        crear_nota(&conn, &ajustes, nueva, "primera nota", cuando(4, 11)).expect("nota 1");
        crear_nota(&conn, &ajustes, nueva, "segunda nota", cuando(4, 12)).expect("nota 2");

        let actual = Huella {
            titulo: Some("Una issue".into()),
            estado: Some("En curso".into()),
            asignado: Some("Ana".into()),
            actualizado: Some("2026-11-04T10:00:00+01:00".into()),
            comentarios_total: 2,
            ultimos: vec![
                Comentario {
                    id: "2".into(),
                    autor: "Luis".into(),
                    mio: false,
                    fecha: "2026-11-04T09:00:00+01:00".into(),
                    extracto: "nuevo".into(),
                },
                Comentario {
                    id: "1".into(),
                    autor: "Ana".into(),
                    mio: false,
                    fecha: "2026-11-03T09:00:00+01:00".into(),
                    extracto: "viejo".into(),
                },
            ],
            tomada: "2026-11-04T10:00:00+01:00".into(),
            ..Huella::default()
        };
        let vista = Huella {
            comentarios_total: 1,
            ultimos: vec![Comentario {
                id: "1".into(),
                autor: "Ana".into(),
                mio: false,
                fecha: "2026-11-03T09:00:00+01:00".into(),
                extracto: "viejo".into(),
            }],
            ..Huella::default()
        };
        conn.execute(
            "UPDATE recurso SET huella_actual = ?2, huella_vista = ?3, estado = 'En curso',
                    cambios = '[{\"tipo\":\"comentarios\",\"n\":1,\"autores\":[\"Luis\"],\"extracto\":\"nuevo\"}]'
              WHERE clave = 'jira:PROJ-1'",
            params![
                "jira:PROJ-1",
                serde_json::to_string(&actual).expect("json"),
                serde_json::to_string(&vista).expect("json")
            ],
        )
        .expect("huellas");

        let detalle = detalle(&conn, nueva).expect("detalle");
        assert_eq!(detalle.entrada.id, nueva);
        assert_eq!(detalle.entrada.clave.as_deref(), Some("PROJ-1"));
        assert_eq!(detalle.entrada.estado.as_deref(), Some("En curso"));
        assert_eq!(
            detalle.entrada.seleccion.as_ref().unwrap().chars().count(),
            280
        );
        assert_eq!(detalle.seleccion.as_ref().unwrap().chars().count(), 300);
        assert_eq!(detalle.notas.len(), 2);
        assert_eq!(detalle.notas[0].texto, "primera nota");
        assert_eq!(detalle.historial.len(), 1);
        assert_eq!(detalle.historial[0].id, vieja);
        assert_eq!(detalle.relacionadas.len(), 1);

        let recurso = detalle.recurso.expect("recurso");
        assert_eq!(recurso.clave, "jira:PROJ-1");
        assert_eq!(recurso.tipo, TipoRecurso::Jira);
        assert_eq!(recurso.estado.as_deref(), Some("En curso"));
        assert_eq!(recurso.asignado.as_deref(), Some("Ana"));
        assert_eq!(recurso.ultimos.len(), 2);
        assert_eq!(recurso.nuevos, vec!["2".to_string()]);
        assert!(!recurso.cambios.is_empty());
    }

    #[test]
    fn notas_recalculan_fts_y_entidades() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let id = texto(&conn, "base", 4, 10);

        let nota = crear_nota(
            &conn,
            &ajustes,
            id,
            "revisar FS#45600 mañana",
            cuando(4, 11),
        )
        .expect("crea");
        assert_eq!(nota.tipo, "nota");
        assert_eq!(nota.entrada_id, id);
        assert!(nota.editada.is_none());
        let entidades: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1 AND valor = 'fs:45600'",
                [id],
                |fila| fila.get(0),
            )
            .expect("entidad");
        assert_eq!(entidades, 1);
        let fts = buscar_fts(&conn, "manana");
        assert!(fts.contains(&id));
        assert!(buscar_fts(&conn, "45600").contains(&id));

        let editada = editar_nota(&conn, &ajustes, nota.id, "revisar PROJ-1896", cuando(4, 12))
            .expect("edita");
        assert!(editada.editada.is_some());
        assert!(
            !buscar_fts(&conn, "manana").contains(&id),
            "el texto viejo sale del FTS"
        );
        assert!(buscar_fts(&conn, "proj").contains(&id));
        assert!(
            !buscar_fts(&conn, "45600").contains(&id),
            "la entidad vieja no puede quedarse en el extra del FTS"
        );
        let fs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1 AND valor = 'fs:45600'",
                [id],
                |fila| fila.get(0),
            )
            .expect("entidad vieja");
        assert_eq!(fs, 0);

        borrar_nota(&conn, &ajustes, nota.id).expect("borra");
        assert!(
            !buscar_fts(&conn, "proj").contains(&id),
            "la referencia de la nota borrada sale del FTS"
        );
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1",
                [id],
                |fila| fila.get(0),
            )
            .expect("notas");
        assert_eq!(total, 0);
        assert!(crear_nota(&conn, &ajustes, id, "   ", cuando(4, 13)).is_err());
        assert!(editar_nota(&conn, &ajustes, 999, "x", cuando(4, 13)).is_err());
        assert!(borrar_nota(&conn, &ajustes, 999).is_err());
    }

    fn buscar_fts(conn: &Connection, texto: &str) -> Vec<i64> {
        let mut consulta = conn
            .prepare("SELECT rowid FROM entrada_fts WHERE entrada_fts MATCH ?1")
            .expect("consulta FTS");
        consulta
            .query_map([texto], |fila| fila.get(0))
            .expect("filas")
            .collect::<Result<Vec<i64>, _>>()
            .expect("ids")
    }

    #[test]
    fn editar_titulo_fijar_borrar_y_restaurar() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let id = texto(&conn, "base", 4, 10);

        editar_titulo(&conn, &ajustes, id, "Gestión de PROJ-1896", cuando(4, 11)).expect("título");
        assert!(buscar_fts(&conn, "gestion").contains(&id));
        let entidad: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1 AND valor = 'jira:PROJ-1896'",
                [id],
                |fila| fila.get(0),
            )
            .expect("entidad");
        assert_eq!(entidad, 1);
        editar_titulo(&conn, &ajustes, id, "Título limpio", cuando(4, 11)).expect("otro título");
        assert!(
            !buscar_fts(&conn, "proj").contains(&id),
            "la entidad del título viejo no queda en el FTS"
        );
        let entidad: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1 AND valor = 'jira:PROJ-1896'",
                [id],
                |fila| fila.get(0),
            )
            .expect("entidad del título viejo");
        assert_eq!(entidad, 0);
        assert!(editar_titulo(&conn, &ajustes, id, "  ", cuando(4, 11)).is_err());
        assert!(editar_titulo(&conn, &ajustes, id, &"t".repeat(1001), cuando(4, 11)).is_err());
        assert!(editar_titulo(&conn, &ajustes, 999, "x", cuando(4, 11)).is_err());

        fijar(&conn, id, true).expect("fija");
        let fijada: i64 = conn
            .query_row("SELECT fijada FROM entrada WHERE id = ?1", [id], |f| {
                f.get(0)
            })
            .expect("fijada");
        assert_eq!(fijada, 1);
        fijar(&conn, id, false).expect("desfija");

        borrar(&conn, id, cuando(4, 12)).expect("borra");
        restaurar(&conn, id).expect("restaura");
        let borrada: Option<String> = conn
            .query_row("SELECT borrada FROM entrada WHERE id = ?1", [id], |f| {
                f.get(0)
            })
            .expect("borrada");
        assert!(borrada.is_none());
    }

    #[test]
    fn borrar_definitivo_solo_desde_la_papelera() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let id = texto(&conn, "una miga viva", 4, 10);
        let error = borrar_definitivo(&conn, id, "x").expect_err("una viva no se borra");
        assert_eq!(
            error.to_string(),
            "Solo se puede borrar definitivamente desde la papelera"
        );
        let (quedan, borrada): (i64, Option<String>) = conn
            .query_row(
                "SELECT COUNT(*), MAX(borrada) FROM entrada WHERE id = ?1",
                [id],
                |f| Ok((f.get(0)?, f.get(1)?)),
            )
            .expect("la entrada sigue");
        assert_eq!(quedan, 1);
        assert!(borrada.is_none(), "intacta y fuera de la papelera");
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entrada_fts WHERE rowid = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("FTS de la viva");
        assert_eq!(fts, 1);
        assert!(borrar_definitivo(&conn, 999, "x").is_err(), "no existe");

        borrar(&conn, id, cuando(4, 11)).expect("a la papelera");
        borrar_definitivo(&conn, id, &sello(&conn, id)).expect("borra definitivo");
        let entradas: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada WHERE id = ?1", [id], |f| {
                f.get(0)
            })
            .expect("ya no está");
        assert_eq!(entradas, 0);
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entrada_fts WHERE rowid = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("FTS borrado");
        assert_eq!(fts, 0);
        assert!(
            borrar_definitivo(&conn, id, "x").is_err(),
            "la segunda vez ya no existe"
        );
    }

    #[test]
    fn borrar_definitivo_arrastra_notas_enlaces_y_fts() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let id = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-77", 4, 10);
        let otra = texto(&conn, "otra miga", 4, 11);
        crear_nota(&conn, &ajustes, id, "una nota", cuando(4, 10)).expect("nota");
        enlazar(&conn, id, otra, cuando(4, 12)).expect("enlaza");
        descartar(&conn, id, otra).expect("descarta");
        let entidades: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("entidades");
        assert!(entidades > 0, "el enlace de Jira deja entidades");

        borrar(&conn, id, cuando(4, 13)).expect("a la papelera");
        borrar_definitivo(&conn, id, &sello(&conn, id)).expect("borra definitivo");

        let notas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("notas");
        assert_eq!(notas, 0);
        let enlaces: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM enlace_manual WHERE a = ?1 OR b = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("enlaces");
        assert_eq!(enlaces, 0);
        let descartes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM descarte WHERE a = ?1 OR b = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("descartes");
        assert_eq!(descartes, 0);
        let entidades: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("entidades");
        assert_eq!(entidades, 0);
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entrada_fts WHERE rowid = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("FTS");
        assert_eq!(fts, 0);
        // La otra miga sigue viva y entera.
        let otra_borrada: Option<String> = conn
            .query_row("SELECT borrada FROM entrada WHERE id = ?1", [otra], |f| {
                f.get(0)
            })
            .expect("la otra sigue");
        assert!(otra_borrada.is_none());
    }

    #[test]
    fn vaciar_papelera_borra_solo_la_papelera() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let viva = texto(&conn, "viva", 4, 9);
        crear_nota(&conn, &ajustes, viva, "nota de la viva", cuando(4, 9)).expect("nota viva");
        let primera = texto(&conn, "papelera uno", 4, 10);
        crear_nota(
            &conn,
            &ajustes,
            primera,
            "nota de la papelera",
            cuando(4, 10),
        )
        .expect("nota");
        let segunda = texto(&conn, "papelera dos", 4, 11);
        borrar(&conn, primera, cuando(4, 12)).expect("a la papelera");
        borrar(&conn, segunda, cuando(4, 12)).expect("a la papelera");

        let borradas = vaciar_papelera(&conn).expect("vacía");
        assert_eq!(borradas, 2);
        let quedan: Vec<i64> = {
            let mut consulta = conn
                .prepare("SELECT id FROM entrada ORDER BY id")
                .expect("consulta");
            consulta
                .query_map([], |fila| fila.get(0))
                .expect("filas")
                .collect::<Result<Vec<_>, _>>()
                .expect("ids")
        };
        assert_eq!(quedan, vec![viva], "solo sigue la viva");
        let notas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1",
                [viva],
                |f| f.get(0),
            )
            .expect("notas de la viva");
        assert_eq!(notas, 1, "la viva conserva sus notas");
        let fts: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada_fts", [], |f| f.get(0))
            .expect("FTS");
        assert_eq!(fts, 1, "solo queda la fila FTS de la viva");
        let papelera = listar(&conn, &filtro(Vista::Papelera), cuando(4, 13)).expect("papelera");
        assert_eq!(papelera.total, 0);
    }

    #[test]
    fn vaciar_papelera_vacia_devuelve_cero() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        assert_eq!(vaciar_papelera(&conn).expect("vacía sin nada"), 0);
        let viva = texto(&conn, "viva", 4, 10);
        assert_eq!(vaciar_papelera(&conn).expect("nada en papelera"), 0);
        let sigue: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada WHERE id = ?1", [viva], |f| {
                f.get(0)
            })
            .expect("la viva sigue");
        assert_eq!(sigue, 1);
    }

    #[test]
    fn restaurar_choca_con_el_indice_unico() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let id = enlace(&conn, "https://ejemplo.com/a", 4, 10);
        borrar(&conn, id, cuando(4, 11)).expect("borra");
        let otra = enlace(&conn, "https://ejemplo.com/a", 4, 12);
        assert_ne!(id, otra);
        let error = restaurar(&conn, id).expect_err("debe chocar");
        assert_eq!(error.to_string(), "Ya hay otra igual ese día");
        // La entrada sigue en papelera.
        let borrada: Option<String> = conn
            .query_row("SELECT borrada FROM entrada WHERE id = ?1", [id], |f| {
                f.get(0)
            })
            .expect("borrada");
        assert!(borrada.is_some());
    }

    #[test]
    fn enlaces_manuales_y_descartes() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let a = texto(&conn, "uno", 4, 10);
        let b = texto(&conn, "dos", 4, 11);

        enlazar(&conn, b, a, cuando(4, 12)).expect("enlaza");
        let pares: Vec<(i64, i64)> = {
            let mut consulta = conn
                .prepare("SELECT a, b FROM enlace_manual")
                .expect("consulta");
            consulta
                .query_map([], |fila| Ok((fila.get(0)?, fila.get(1)?)))
                .expect("filas")
                .collect::<Result<Vec<_>, _>>()
                .expect("pares")
        };
        assert_eq!(pares, vec![(a, b)], "el par se normaliza a < b");
        desenlazar(&conn, b, a).expect("desenlaza");
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM enlace_manual", [], |f| f.get(0))
            .expect("total");
        assert_eq!(total, 0);

        descartar(&conn, b, a).expect("descarta");
        let pares: Vec<(i64, i64)> = {
            let mut consulta = conn.prepare("SELECT a, b FROM descarte").expect("consulta");
            consulta
                .query_map([], |fila| Ok((fila.get(0)?, fila.get(1)?)))
                .expect("filas")
                .collect::<Result<Vec<_>, _>>()
                .expect("pares")
        };
        assert_eq!(pares, vec![(a, b)]);

        assert!(enlazar(&conn, a, a, cuando(4, 12)).is_err());
        assert!(descartar(&conn, a, a).is_err());
        assert!(enlazar(&conn, 999, a, cuando(4, 12)).is_err());
        assert!(descartar(&conn, 999, a).is_err());
    }

    #[test]
    fn seguir_y_marcar_visto() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let id = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        let sin_recurso = texto(&conn, "sin recurso", 4, 11);

        seguir(&conn, id, false).expect("deja de seguir");
        let seguir_valor: i64 = conn
            .query_row(
                "SELECT seguir FROM recurso WHERE clave = 'jira:PROJ-1'",
                [],
                |f| f.get(0),
            )
            .expect("seguir");
        assert_eq!(seguir_valor, 0);
        assert!(seguir(&conn, sin_recurso, true).is_err());
        assert!(seguir(&conn, 999, true).is_err());

        // Con huella, marcar visto la copia.
        conn.execute(
            "UPDATE recurso SET huella_actual = '{\"titulo\":\"x\"}' WHERE clave = 'jira:PROJ-1'",
            [],
        )
        .expect("huella");
        marcar_visto_entrada(&conn, id, cuando(4, 12)).expect("marca");
        let (vista, cambios): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT huella_vista, cambios FROM recurso WHERE clave = 'jira:PROJ-1'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("recurso");
        assert!(vista.is_some());
        assert!(cambios.is_none());
        marcar_visto_entrada(&conn, sin_recurso, cuando(4, 12)).expect("sin recurso no falla");
        assert!(marcar_visto_entrada(&conn, 999, cuando(4, 12)).is_err());
    }

    #[test]
    fn marcar_todo_visto_y_contar() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        enlace(&conn, "https://empresa.freshservice.com/a/tickets/1", 4, 11);
        conn.execute(
            "UPDATE recurso SET cambios = '[]' WHERE clave = 'jira:PROJ-1'",
            [],
        )
        .expect("cambios 1");
        conn.execute("UPDATE recurso SET cambios = '[]' WHERE clave = 'fs:1'", [])
            .expect("cambios 2");
        assert_eq!(contar_cambios(&conn).expect("cuenta"), 2);
        assert_eq!(marcar_todo_visto(&conn, cuando(4, 12)).expect("marca"), 2);
        assert_eq!(contar_cambios(&conn).expect("cuenta"), 0);
    }

    #[test]
    fn estado_de_una_url() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();

        let vacio = estado_url(
            &conn,
            "https://empresa.atlassian.net/browse/PROJ-1",
            &ajustes,
            cuando(4, 10),
        )
        .expect("vacío");
        assert!(!vacio.guardada_hoy);
        assert_eq!(vacio.id, None);
        assert_eq!(vacio.veces, 0);

        let dia3 = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 3, 10);
        let dia4 = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1", 4, 10);
        let estado = estado_url(
            &conn,
            "https://empresa.atlassian.net/browse/PROJ-1?focusedCommentId=1",
            &ajustes,
            cuando(4, 12),
        )
        .expect("estado");
        assert!(estado.guardada_hoy);
        assert_eq!(estado.id, Some(dia4));
        assert_eq!(estado.veces, 2);
        assert_eq!(estado.relacionadas, 1);
        assert_ne!(estado.id, Some(dia3));

        // Una web normal se cuenta por url_norm.
        enlace(&conn, "https://ejemplo.com/x?utm_source=1", 4, 11);
        let web = estado_url(&conn, "https://ejemplo.com/x", &ajustes, cuando(4, 12)).expect("web");
        assert!(web.guardada_hoy);
        assert_eq!(web.veces, 1);
    }

    #[test]
    fn app_y_proyecto_preferentes() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let id = enlace(
            &conn,
            "https://gitlab.com/grupo/informes-web/-/merge_requests/1",
            4,
            10,
        );
        let vista = entrada_vista(&conn, id).expect("vista");
        assert_eq!(vista.app.as_deref(), Some("informes-web"));
        assert_eq!(vista.proyecto.as_deref(), Some("grupo/informes-web"));
        assert_eq!(vista.clave.as_deref(), Some("!1"));
        assert_eq!(vista.host, None, "solo las webs llevan host");

        // El recurso remoto manda si la URL no da app.
        let fs = enlace(&conn, "https://empresa.freshservice.com/a/tickets/1", 4, 11);
        conn.execute("UPDATE recurso SET app = 'zoho' WHERE clave = 'fs:1'", [])
            .expect("app remota");
        let vista = entrada_vista(&conn, fs).expect("vista fs");
        assert_eq!(vista.app.as_deref(), Some("zoho"));

        // Una web ajena no hereda app ni proyecto de GitLab por su path.
        let ajena = enlace(
            &conn,
            "https://ejemplo.org/grupo/falsa-app/-/tree/main",
            4,
            13,
        );
        let vista = entrada_vista(&conn, ajena).expect("vista web ajena");
        assert_eq!(vista.fuente, Fuente::Web);
        assert_eq!(vista.app, None, "la app de una web ajena no es de GitLab");
        assert_eq!(vista.proyecto, None);
        assert_eq!(vista.host.as_deref(), Some("ejemplo.org"));

        // Y el texto es el último recurso.
        let suelta = texto(&conn, "hablando de PROJ-1", 4, 12);
        conn.execute(
            "INSERT OR IGNORE INTO entidad (entrada_id, valor, fuente) VALUES (?1, 'app:desde-texto', 'texto')",
            [suelta],
        )
        .expect("entidad app");
        let vista = entrada_vista(&conn, suelta).expect("vista texto");
        assert_eq!(vista.app.as_deref(), Some("desde-texto"));
    }

    #[test]
    fn seleccion_y_ultima_nota_recortadas() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let id = texto(&conn, &"a".repeat(400), 4, 10);
        crear_nota(
            &conn,
            &Ajustes::default(),
            id,
            &"n".repeat(200),
            cuando(4, 11),
        )
        .expect("nota");
        let vista = entrada_vista(&conn, id).expect("vista");
        assert_eq!(vista.seleccion.as_ref().unwrap().chars().count(), 280);
        assert_eq!(vista.ultima_nota.as_ref().unwrap().chars().count(), 140);
        assert_eq!(vista.notas, 1);
    }
}
