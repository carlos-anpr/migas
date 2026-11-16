//! Guardar capturas con las reglas de duplicado y mantener FTS + entidades.

use crate::error::{Error, Resultado};
use crate::model::{
    Ajustes, Fuente, NuevaEntrada, Recurso, ResultadoCaptura, TipoEntrada, TipoRecurso,
};
use crate::{db, entidades, recurso, relaciones};
use chrono::{DateTime, Local};
use rusqlite::{Connection, OptionalExtension, params};
use url::Url;

/// Tope de selección y notas.
const MAX_TEXTO: usize = 20_000;
/// Tope del título.
const MAX_TITULO: usize = 1_000;
/// Tope de la URL.
const MAX_URL: usize = 4_096;
/// Ventana del antiduplicado de texto.
const SEGUNDOS_DUPLICADO: i64 = 10;

/// Guarda una captura aplicando todas las reglas del contrato, en una transacción.
pub fn capturar(
    conn: &Connection,
    nueva: &NuevaEntrada,
    ajustes: &Ajustes,
    ahora: DateTime<Local>,
) -> Resultado<ResultadoCaptura> {
    let tipo = nueva.tipo;
    let titulo = validar_titulo(nueva.titulo.as_deref().unwrap_or(""))?;
    let seleccion = nueva
        .seleccion
        .as_deref()
        .map(|texto| recortar(texto, MAX_TEXTO))
        .filter(|texto| !texto.trim().is_empty());
    let nota = nueva
        .nota
        .as_deref()
        .map(|texto| recortar(texto, MAX_TEXTO))
        .filter(|texto| !texto.trim().is_empty());

    let (url, reconocido) = match tipo {
        TipoEntrada::Enlace => {
            let url = validar_url(nueva.url.as_deref())?;
            let reconocido = recurso::reconocer(&url, ajustes);
            (Some(url), reconocido)
        }
        // `texto` y `nota` son capturas sin URL (contrato §Captura).
        TipoEntrada::Texto | TipoEntrada::Nota => (None, None),
    };
    let url_norm = match (&url, &reconocido) {
        (Some(_), Some(reconocido)) => Some(reconocido.url.clone()),
        (Some(url), None) => Some(recurso::normalizar(url)),
        (None, _) => None,
    };

    let ctx = entidades::Contexto::cargar(conn, ajustes)?;
    let tx = db::transaccion_inmediata(conn)?;
    let resultado = (|| -> Resultado<ResultadoCaptura> {
        let dia = ahora.format("%Y-%m-%d").to_string();
        let creada = ahora.to_rfc3339();

        if tipo == TipoEntrada::Enlace
            && let Some(url_norm) = &url_norm
        {
            let existente = tx
                .query_row(
                    "SELECT id, titulo, seleccion, recurso FROM entrada
                      WHERE dia = ?1 AND url_norm = ?2 AND borrada IS NULL",
                    params![dia, url_norm],
                    |fila| {
                        Ok((
                            fila.get::<_, i64>(0)?,
                            fila.get::<_, String>(1)?,
                            fila.get::<_, Option<String>>(2)?,
                            fila.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((id, titulo_viejo, seleccion_vieja, recurso_viejo)) = existente {
                tx.execute(
                    "UPDATE entrada SET capturas = capturas + 1, actualizada = ?2 WHERE id = ?1",
                    params![id, creada],
                )?;
                let titulo_final = if titulo_viejo.trim().is_empty() && !titulo.is_empty() {
                    tx.execute(
                        "UPDATE entrada SET titulo = ?2 WHERE id = ?1",
                        params![id, titulo],
                    )?;
                    titulo.clone()
                } else {
                    titulo_viejo
                };
                if let Some(nueva_seleccion) = &seleccion {
                    match seleccion_vieja.as_deref() {
                        Some(vieja) if vieja == nueva_seleccion => {}
                        Some(vieja) if !vieja.trim().is_empty() => {
                            insertar_nota(&tx, id, "cita", nueva_seleccion, &creada)?;
                        }
                        _ => {
                            tx.execute(
                                "UPDATE entrada SET seleccion = ?2 WHERE id = ?1",
                                params![id, nueva_seleccion],
                            )?;
                        }
                    }
                }
                if let Some(nota) = &nota {
                    insertar_nota(&tx, id, "nota", nota, &creada)?;
                }
                if let Some(reconocido) = &reconocido {
                    asegurar_recurso(&tx, reconocido, url.as_deref())?;
                    tx.execute(
                        "UPDATE entrada SET recurso = ?2 WHERE id = ?1 AND (recurso IS NULL OR recurso <> ?2)",
                        params![id, reconocido.clave],
                    )?;
                    recurso::marcar_visto_si_nuevo(&tx, &reconocido.clave, ahora)?;
                }
                let clave = reconocido
                    .as_ref()
                    .map(|recurso| recurso.clave.clone())
                    .or(recurso_viejo);
                entidades::recalcular(&tx, id, &ctx)?;
                reindexar(&tx, id)?;
                return Ok(ResultadoCaptura {
                    id,
                    duplicada: true,
                    titulo: titulo_final,
                    fuente: leer_fuente(&tx, id)?,
                    cambios: contar_cambios(&tx, clave.as_deref())?,
                    clave: clave.as_deref().map(recurso::visible),
                    relacionadas: relaciones::contar(&tx, id)?,
                });
            }
        }

        // Texto idéntico al de otra entrada del mismo día creada hace menos de 10 s.
        if matches!(tipo, TipoEntrada::Texto | TipoEntrada::Nota) {
            let texto = seleccion.as_deref().or(nota.as_deref());
            if let Some(texto) = texto
                && let Some(id) = buscar_texto_repetido(&tx, &dia, texto, ahora)?
            {
                // La nota adjunta sí es información nueva: se añade a la entrada existente.
                if let Some(nota) = &nota
                    && !nota_ya_existe(&tx, id, nota)?
                {
                    insertar_nota(&tx, id, "nota", nota, &creada)?;
                    tx.execute(
                        "UPDATE entrada SET actualizada = ?2 WHERE id = ?1",
                        params![id, creada],
                    )?;
                    entidades::recalcular(&tx, id, &ctx)?;
                    reindexar(&tx, id)?;
                }
                return Ok(ResultadoCaptura {
                    id,
                    duplicada: true,
                    titulo: leer_titulo(&tx, id)?,
                    fuente: leer_fuente(&tx, id)?,
                    clave: leer_clave(&tx, id)?.as_deref().map(recurso::visible),
                    cambios: 0,
                    relacionadas: relaciones::contar(&tx, id)?,
                });
            }
        }

        let titulo_final = if titulo.is_empty() {
            titulo_por_defecto(tipo, url.as_deref(), seleccion.as_deref(), nota.as_deref())
        } else {
            titulo
        };
        let fuente = match tipo {
            TipoEntrada::Enlace => recurso::fuente_de_url(url.as_deref().unwrap_or(""), ajustes),
            TipoEntrada::Texto => Fuente::Texto,
            TipoEntrada::Nota => Fuente::Nota,
        };
        tx.execute(
            "INSERT INTO entrada
                (dia, creada, actualizada, tipo, origen, fuente, url, url_norm, titulo,
                 seleccion, recurso, fijada, capturas, borrada)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 0, 1, NULL)",
            params![
                dia,
                creada,
                tipo.como_str(),
                nueva.origen.como_str(),
                fuente_como_str(fuente),
                url,
                url_norm,
                titulo_final,
                seleccion,
            ],
        )?;
        let id = tx.last_insert_rowid();

        if let Some(reconocido) = &reconocido {
            asegurar_recurso(&tx, reconocido, url.as_deref())?;
            tx.execute(
                "UPDATE entrada SET recurso = ?2 WHERE id = ?1",
                params![id, reconocido.clave],
            )?;
            recurso::marcar_visto_si_nuevo(&tx, &reconocido.clave, ahora)?;
        }
        if let Some(nota) = &nota {
            insertar_nota(&tx, id, "nota", nota, &creada)?;
        }
        entidades::recalcular(&tx, id, &ctx)?;
        reindexar(&tx, id)?;

        let clave = reconocido.map(|recurso| recurso.clave);
        Ok(ResultadoCaptura {
            id,
            duplicada: false,
            titulo: leer_titulo(&tx, id)?,
            fuente: leer_fuente(&tx, id)?,
            cambios: contar_cambios(&tx, clave.as_deref())?,
            clave: clave.as_deref().map(recurso::visible),
            relacionadas: relaciones::contar(&tx, id)?,
        })
    })();
    let resultado = resultado?;
    tx.commit()?;
    Ok(resultado)
}

/// Fila de datos que alimenta el índice FTS.
struct FilaFts {
    titulo: String,
    url: Option<String>,
    seleccion: Option<String>,
    clave: Option<String>,
    estado: Option<String>,
    app: Option<String>,
}

/// Rehace la fila de FTS de una entrada (`rowid = entrada.id`).
pub fn reindexar(conn: &Connection, id: i64) -> Resultado<()> {
    conn.execute("DELETE FROM entrada_fts WHERE rowid = ?1", [id])?;
    let fila: Option<FilaFts> = conn
        .query_row(
            "SELECT e.titulo, e.url, e.seleccion, e.recurso, r.estado, r.app
               FROM entrada e LEFT JOIN recurso r ON r.clave = e.recurso
              WHERE e.id = ?1",
            [id],
            |fila| {
                Ok(FilaFts {
                    titulo: fila.get(0)?,
                    url: fila.get(1)?,
                    seleccion: fila.get(2)?,
                    clave: fila.get(3)?,
                    estado: fila.get(4)?,
                    app: fila.get(5)?,
                })
            },
        )
        .optional()?;
    let Some(fila) = fila else {
        return Ok(());
    };
    let notas = {
        let mut consulta =
            conn.prepare("SELECT texto FROM nota WHERE entrada_id = ?1 ORDER BY creada, id")?;
        consulta
            .query_map([id], |fila| fila.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?
            .join(" ")
    };
    let entidades = {
        let mut consulta =
            conn.prepare("SELECT valor FROM entidad WHERE entrada_id = ?1 ORDER BY valor")?;
        consulta
            .query_map([id], |fila| fila.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?
            .join(" ")
    };
    let extra = [
        fila.clave
            .map(|clave| recurso::visible(&clave))
            .unwrap_or_default(),
        fila.estado.unwrap_or_default(),
        fila.app.unwrap_or_default(),
        entidades,
    ]
    .into_iter()
    .filter(|parte| !parte.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    conn.execute(
        "INSERT INTO entrada_fts (rowid, titulo, url, seleccion, notas, extra)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            fila.titulo,
            fila.url.unwrap_or_default(),
            fila.seleccion.unwrap_or_default(),
            notas,
            extra
        ],
    )?;
    Ok(())
}

fn validar_titulo(titulo: &str) -> Resultado<String> {
    let titulo = titulo.trim();
    if titulo.chars().count() > MAX_TITULO {
        return Err(Error::Validacion("El título es demasiado largo".into()));
    }
    Ok(titulo.to_string())
}

fn validar_url(url: Option<&str>) -> Resultado<String> {
    let url = url
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| Error::Validacion("Falta la URL".into()))?;
    if url.chars().count() > MAX_URL {
        return Err(Error::Validacion("La URL es demasiado larga".into()));
    }
    let parseada = Url::parse(url).map_err(|_| Error::Validacion("La URL no es válida".into()))?;
    if !matches!(parseada.scheme(), "http" | "https") {
        return Err(Error::Validacion("La URL no es válida".into()));
    }
    Ok(url.to_string())
}

fn recortar(texto: &str, maximo: usize) -> String {
    texto.chars().take(maximo).collect()
}

fn titulo_por_defecto(
    tipo: TipoEntrada,
    url: Option<&str>,
    seleccion: Option<&str>,
    nota: Option<&str>,
) -> String {
    let titulo = match tipo {
        TipoEntrada::Enlace => url
            .and_then(|url| Url::parse(url).ok())
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_default(),
        TipoEntrada::Texto => primera_linea(seleccion.unwrap_or(""), 80),
        TipoEntrada::Nota => primera_linea(nota.or(seleccion).unwrap_or(""), 80),
    };
    if titulo.is_empty() {
        "Sin título".to_string()
    } else {
        titulo
    }
}

/// Primeras `maximo` letras en una sola línea (espacios colapsados).
fn primera_linea(texto: &str, maximo: usize) -> String {
    let linea = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    recortar(&linea, maximo)
}

fn asegurar_recurso(
    conn: &Connection,
    recurso: &Recurso,
    url_original: Option<&str>,
) -> Resultado<()> {
    // `Recurso.url` (el modelo) es la canónica que usa `url_norm` como identidad. Para
    // issues/WI la columna guarda la primera URL original que llegó (`issues` o `work_items`).
    let url = if recurso.tipo == TipoRecurso::Issue {
        url_original
            .filter(|url| !url.is_empty())
            .unwrap_or(&recurso.url)
    } else {
        &recurso.url
    };
    conn.execute(
        "INSERT OR IGNORE INTO recurso (clave, tipo, url, proyecto) VALUES (?1, ?2, ?3, ?4)",
        params![
            recurso.clave,
            tipo_recurso_como_str(recurso.tipo),
            url,
            recurso.proyecto
        ],
    )?;
    Ok(())
}

fn insertar_nota(
    conn: &Connection,
    entrada_id: i64,
    tipo: &str,
    texto: &str,
    creada: &str,
) -> Resultado<()> {
    conn.execute(
        "INSERT INTO nota (entrada_id, creada, tipo, texto) VALUES (?1, ?2, ?3, ?4)",
        params![entrada_id, creada, tipo, texto],
    )?;
    Ok(())
}

fn buscar_texto_repetido(
    conn: &Connection,
    dia: &str,
    texto: &str,
    ahora: DateTime<Local>,
) -> Resultado<Option<i64>> {
    let desde = (ahora - chrono::Duration::seconds(SEGUNDOS_DUPLICADO)).to_rfc3339();
    Ok(conn
        .query_row(
            "SELECT e.id FROM entrada e
              WHERE e.dia = ?1 AND e.borrada IS NULL AND e.tipo IN ('texto', 'nota')
                AND julianday(e.creada) >= julianday(?2)
                AND (e.seleccion = ?3
                     OR EXISTS (SELECT 1 FROM nota n WHERE n.entrada_id = e.id AND n.texto = ?3))
              ORDER BY e.id DESC LIMIT 1",
            params![dia, desde, texto],
            |fila| fila.get(0),
        )
        .optional()?)
}

/// `true` si la entrada ya tiene una nota con ese texto exacto.
fn nota_ya_existe(conn: &Connection, entrada_id: i64, texto: &str) -> Resultado<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM nota WHERE entrada_id = ?1 AND texto = ?2 LIMIT 1",
            params![entrada_id, texto],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn leer_titulo(conn: &Connection, id: i64) -> Resultado<String> {
    Ok(
        conn.query_row("SELECT titulo FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })?,
    )
}

fn leer_fuente(conn: &Connection, id: i64) -> Resultado<Fuente> {
    let fuente: String =
        conn.query_row("SELECT fuente FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })?;
    Ok(fuente_de_str(&fuente))
}

fn leer_clave(conn: &Connection, id: i64) -> Resultado<Option<String>> {
    Ok(
        conn.query_row("SELECT recurso FROM entrada WHERE id = ?1", [id], |fila| {
            fila.get(0)
        })?,
    )
}

fn contar_cambios(conn: &Connection, clave: Option<&str>) -> Resultado<u32> {
    let Some(clave) = clave else {
        return Ok(0);
    };
    let json: Option<String> = conn
        .query_row(
            "SELECT cambios FROM recurso WHERE clave = ?1",
            [clave],
            |fila| fila.get(0),
        )
        .optional()?
        .flatten();
    let total = json
        .as_deref()
        .and_then(|texto| serde_json::from_str::<Vec<serde_json::Value>>(texto).ok())
        .map_or(0, |cambios| cambios.len());
    Ok(u32::try_from(total).unwrap_or(0))
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

fn tipo_recurso_como_str(tipo: crate::model::TipoRecurso) -> &'static str {
    match tipo {
        crate::model::TipoRecurso::Jira => "jira",
        crate::model::TipoRecurso::Mr => "mr",
        crate::model::TipoRecurso::Issue => "issue",
        crate::model::TipoRecurso::Fs => "fs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::{Origen, TipoEntrada};
    use chrono::TimeZone;

    fn bd(dir: &std::path::Path) -> Connection {
        db::abrir(&dir.join("migas.db")).expect("BD de prueba")
    }

    fn cuando(dia: u32, hora: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, dia, hora, 0, 0).unwrap()
    }

    fn enlace(url: &str, titulo: Option<&str>, seleccion: Option<&str>) -> NuevaEntrada {
        NuevaEntrada {
            tipo: TipoEntrada::Enlace,
            origen: Origen::Navegador,
            url: Some(url.into()),
            titulo: titulo.map(str::to_string),
            seleccion: seleccion.map(str::to_string),
            nota: None,
        }
    }

    fn texto(texto: &str) -> NuevaEntrada {
        NuevaEntrada {
            tipo: TipoEntrada::Texto,
            origen: Origen::Global,
            url: None,
            titulo: None,
            seleccion: Some(texto.into()),
            nota: None,
        }
    }

    #[test]
    fn captura_un_enlace_nuevo() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let r = capturar(
            &conn,
            &enlace(
                "https://ejemplo.com/pagina?utm_source=x",
                Some("Una página"),
                None,
            ),
            &ajustes,
            cuando(4, 10),
        )
        .expect("captura");
        assert!(!r.duplicada);
        assert_eq!(r.titulo, "Una página");
        assert_eq!(r.fuente, Fuente::Web);
        assert_eq!(r.clave, None);
        assert_eq!(r.cambios, 0);
        assert_eq!(r.relacionadas, 0);
        let (dia, url_norm, tipo, origen, fuente, capturas): (
            String,
            String,
            String,
            String,
            String,
            i64,
        ) = conn
            .query_row(
                "SELECT dia, url_norm, tipo, origen, fuente, capturas FROM entrada WHERE id = ?1",
                [r.id],
                |fila| {
                    Ok((
                        fila.get(0)?,
                        fila.get(1)?,
                        fila.get(2)?,
                        fila.get(3)?,
                        fila.get(4)?,
                        fila.get(5)?,
                    ))
                },
            )
            .expect("lee entrada");
        assert_eq!(dia, "2026-11-04");
        assert_eq!(url_norm, "https://ejemplo.com/pagina");
        assert_eq!(tipo, "enlace");
        assert_eq!(origen, "navegador");
        assert_eq!(fuente, "web");
        assert_eq!(capturas, 1);
    }

    #[test]
    fn titulo_por_defecto_es_el_host() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let r = capturar(
            &conn,
            &enlace("https://gitlab.com/grupo/proy", None, None),
            &ajustes,
            cuando(4, 10),
        )
        .expect("captura");
        assert_eq!(r.titulo, "gitlab.com");
        assert_eq!(r.fuente, Fuente::Gitlab);
    }

    #[test]
    fn el_mismo_enlace_el_mismo_dia_se_fusiona() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let primera = capturar(
            &conn,
            &enlace("https://ejemplo.com/a", Some("Uno"), Some("texto uno")),
            &ajustes,
            cuando(4, 10),
        )
        .expect("primera");
        let segunda = capturar(
            &conn,
            &enlace("https://ejemplo.com/a/", Some("Dos"), Some("texto uno")),
            &ajustes,
            cuando(4, 11),
        )
        .expect("segunda");
        assert!(segunda.duplicada);
        assert_eq!(segunda.id, primera.id);
        assert_eq!(
            segunda.titulo, "Uno",
            "el título no se pisa si ya había uno"
        );
        let capturas: i64 = conn
            .query_row(
                "SELECT capturas FROM entrada WHERE id = ?1",
                [primera.id],
                |f| f.get(0),
            )
            .expect("capturas");
        assert_eq!(capturas, 2);
        let notas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1",
                [primera.id],
                |f| f.get(0),
            )
            .expect("notas");
        assert_eq!(notas, 0, "la selección idéntica no crea cita");

        // Una selección distinta se añade como cita.
        let tercera = capturar(
            &conn,
            &enlace("https://ejemplo.com/a", None, Some("otra selección")),
            &ajustes,
            cuando(4, 12),
        )
        .expect("tercera");
        assert!(tercera.duplicada);
        let citas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1 AND tipo = 'cita'",
                [primera.id],
                |f| f.get(0),
            )
            .expect("citas");
        assert_eq!(citas, 1);
    }

    #[test]
    fn al_repetir_rellena_titulo_y_seleccion_vacios_y_anade_nota() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        conn.execute(
            "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, url, url_norm, titulo)
             VALUES ('2026-11-04', '2026-11-04T09:00:00+01:00', '2026-11-04T09:00:00+01:00',
                     'enlace', 'navegador', 'web', 'https://ejemplo.com/b', 'https://ejemplo.com/b', '')",
            [],
        )
        .expect("entrada vacía");
        let id = conn.last_insert_rowid();

        let mut nueva = enlace(
            "https://ejemplo.com/b",
            Some("Título que llega tarde"),
            Some("selección nueva"),
        );
        nueva.nota = Some("nota al repetir".into());
        let r = capturar(&conn, &nueva, &ajustes, cuando(4, 10)).expect("repite");
        assert!(r.duplicada);
        assert_eq!(r.id, id);
        assert_eq!(r.titulo, "Título que llega tarde");
        let (titulo, seleccion): (String, Option<String>) = conn
            .query_row(
                "SELECT titulo, seleccion FROM entrada WHERE id = ?1",
                [id],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("entrada");
        assert_eq!(titulo, "Título que llega tarde");
        assert_eq!(seleccion.as_deref(), Some("selección nueva"));
        let (notas, citas): (i64, i64) = conn
            .query_row(
                "SELECT SUM(tipo = 'nota'), SUM(tipo = 'cita') FROM nota WHERE entrada_id = ?1",
                [id],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("notas");
        assert_eq!(notas, 1);
        assert_eq!(citas, 0, "la selección no existía: no es una cita");
    }

    #[test]
    fn el_mismo_enlace_al_dia_siguiente_es_otra_entrada() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let primera = capturar(
            &conn,
            &enlace("https://ejemplo.com/a", None, None),
            &ajustes,
            cuando(4, 10),
        )
        .expect("primera");
        let segunda = capturar(
            &conn,
            &enlace("https://ejemplo.com/a", None, None),
            &ajustes,
            cuando(5, 10),
        )
        .expect("segunda");
        assert!(!segunda.duplicada);
        assert_ne!(segunda.id, primera.id);
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada", [], |f| f.get(0))
            .expect("total");
        assert_eq!(total, 2);
    }

    #[test]
    fn un_recurso_se_guarda_y_se_marca_visto() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let r = capturar(
            &conn,
            &enlace(
                "https://empresa.atlassian.net/browse/PROJ-1896?focusedCommentId=3",
                None,
                None,
            ),
            &ajustes,
            cuando(4, 10),
        )
        .expect("captura");
        assert_eq!(r.clave.as_deref(), Some("PROJ-1896"));
        assert_eq!(r.fuente, Fuente::Jira);
        let (tipo, url, url_norm, recurso_clave, pendiente, visto): (
            String,
            String,
            String,
            String,
            i64,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT r.tipo, r.url, r.url, r.clave, r.pendiente_visto, r.visto
                   FROM recurso r WHERE r.clave = 'jira:PROJ-1896'",
                [],
                |fila| {
                    Ok((
                        fila.get(0)?,
                        fila.get(1)?,
                        fila.get(2)?,
                        fila.get(3)?,
                        fila.get(4)?,
                        fila.get(5)?,
                    ))
                },
            )
            .expect("recurso");
        assert_eq!(tipo, "jira");
        assert_eq!(url, "https://empresa.atlassian.net/browse/PROJ-1896");
        assert_eq!(url_norm, url);
        assert_eq!(recurso_clave, "jira:PROJ-1896");
        assert_eq!(pendiente, 1);
        assert!(visto.is_some());
    }

    #[test]
    fn texto_duplicado_solo_dentro_de_10_segundos() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let primera =
            capturar(&conn, &texto("una miga de texto"), &ajustes, cuando(4, 10)).expect("primera");
        let repetida = capturar(&conn, &texto("una miga de texto"), &ajustes, cuando(4, 10))
            .expect("repetida");
        assert!(repetida.duplicada);
        assert_eq!(repetida.id, primera.id);

        // A los 11 segundos ya es otra entrada.
        let mut despues = cuando(4, 10);
        despues += chrono::Duration::seconds(11);
        let otra = capturar(&conn, &texto("una miga de texto"), &ajustes, despues).expect("otra");
        assert!(!otra.duplicada);
        assert_ne!(otra.id, primera.id);
    }

    #[test]
    fn la_nota_del_duplicado_de_texto_se_guarda_y_recalcula() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let primera =
            capturar(&conn, &texto("misma seleccion"), &ajustes, cuando(4, 10)).expect("primera");
        let mut segunda = texto("misma seleccion");
        segunda.nota = Some("revisar FS#45600".into());

        let repetida = capturar(&conn, &segunda, &ajustes, cuando(4, 10)).expect("duplicada");
        assert!(repetida.duplicada);
        assert_eq!(repetida.id, primera.id);
        let notas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1 AND texto = 'revisar FS#45600'",
                [primera.id],
                |fila| fila.get(0),
            )
            .expect("notas");
        assert_eq!(notas, 1, "la nota se añade a la entrada existente");

        let entidad: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1 AND valor = 'fs:45600'",
                [primera.id],
                |fila| fila.get(0),
            )
            .expect("entidad");
        assert_eq!(entidad, 1, "la entidad se recalcula con la nota nueva");
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entrada_fts WHERE entrada_fts MATCH '\"45600\"*'",
                [],
                |fila| fila.get(0),
            )
            .expect("FTS");
        assert_eq!(fts, 1, "el FTS se rehace después de las entidades");

        // Repetir la misma nota no la duplica.
        capturar(&conn, &segunda, &ajustes, cuando(4, 10)).expect("otra vez");
        let notas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1 AND texto = 'revisar FS#45600'",
                [primera.id],
                |fila| fila.get(0),
            )
            .expect("notas");
        assert_eq!(notas, 1);
    }

    #[test]
    fn la_captura_espera_al_escritor_en_vez_de_fallar() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let conn = db::abrir(&ruta).expect("BD");
        let otro = db::abrir(&ruta).expect("otra conexión");
        let (aviso, recibido) = std::sync::mpsc::channel();
        let escritor = std::thread::spawn(move || {
            otro.execute_batch("BEGIN IMMEDIATE").expect("bloqueo");
            aviso.send(()).expect("aviso");
            std::thread::sleep(std::time::Duration::from_millis(500));
            otro.execute_batch("COMMIT").expect("commit");
        });

        recibido.recv().expect("bloqueo tomado");
        let inicio = std::time::Instant::now();
        let resultado = capturar(
            &conn,
            &enlace("https://ejemplo.com/immediate", None, None),
            &Ajustes::default(),
            cuando(4, 10),
        );
        let espera = inicio.elapsed();
        escritor.join().expect("hilo");

        assert!(
            resultado.is_ok(),
            "debería esperar al escritor: {:?}",
            resultado.err()
        );
        assert!(
            espera >= std::time::Duration::from_millis(400),
            "esperó {espera:?}"
        );
    }

    #[test]
    fn issues_y_work_items_el_mismo_dia_son_la_misma_entrada() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let base = "https://gitlab.com/equipo/despliegues";
        let clave = "issue:equipo/despliegues#9";

        let issues = capturar(
            &conn,
            &enlace(&format!("{base}/-/issues/9"), Some("Issue"), None),
            &ajustes,
            cuando(4, 10),
        )
        .expect("issues");
        let work_items = capturar(
            &conn,
            &enlace(&format!("{base}/-/work_items/9"), Some("WI"), None),
            &ajustes,
            cuando(4, 11),
        )
        .expect("work_items");
        assert!(work_items.duplicada);
        assert_eq!(work_items.id, issues.id);
        let (url, url_norm, recurso): (String, String, String) = conn
            .query_row(
                "SELECT url, url_norm, recurso FROM entrada WHERE id = ?1",
                [issues.id],
                |fila| Ok((fila.get(0)?, fila.get(1)?, fila.get(2)?)),
            )
            .expect("entrada");
        assert_eq!(
            url,
            format!("{base}/-/issues/9"),
            "se guarda la URL original"
        );
        assert_eq!(url_norm, format!("{base}/-/issues/9"));
        assert_eq!(recurso, clave);
        let recurso_url: String = conn
            .query_row(
                "SELECT url FROM recurso WHERE clave = ?1",
                [clave],
                |fila| fila.get(0),
            )
            .expect("recurso");
        assert_eq!(recurso_url, format!("{base}/-/issues/9"));

        // En el orden inverso, la URL original es la de work_items, pero la identidad no cambia.
        let work_first = capturar(
            &conn,
            &enlace(&format!("{base}/-/work_items/7"), None, None),
            &ajustes,
            cuando(4, 12),
        )
        .expect("work_items primero");
        let issues_second = capturar(
            &conn,
            &enlace(&format!("{base}/-/issues/7"), None, None),
            &ajustes,
            cuando(4, 13),
        )
        .expect("issues después");
        assert!(issues_second.duplicada);
        assert_eq!(issues_second.id, work_first.id);
        let (url, url_norm): (String, String) = conn
            .query_row(
                "SELECT url, url_norm FROM entrada WHERE id = ?1",
                [work_first.id],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("entrada inversa");
        assert_eq!(url, format!("{base}/-/work_items/7"));
        assert_eq!(url_norm, format!("{base}/-/issues/7"));
        let recurso_url: String = conn
            .query_row(
                "SELECT url FROM recurso WHERE clave = ?1",
                ["issue:equipo/despliegues#7"],
                |fila| fila.get(0),
            )
            .expect("recurso inverso");
        assert_eq!(
            recurso_url,
            format!("{base}/-/work_items/7"),
            "recurso.url es la primera que llegó"
        );
    }

    #[test]
    fn titulo_de_texto_y_de_nota() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let largo = format!("  Primera\n\nlínea   con   espacios {}", "x".repeat(200));
        let r = capturar(&conn, &texto(&largo), &ajustes, cuando(4, 10)).expect("texto");
        assert!(r.titulo.starts_with("Primera línea con espacios xxx"));
        assert!(r.titulo.chars().count() <= 80);
        assert!(!r.titulo.contains('\n'));

        let mut nota = texto("ignorado");
        nota.tipo = TipoEntrada::Nota;
        nota.seleccion = None;
        nota.nota = Some("Nota suelta para el diario".into());
        let r = capturar(&conn, &nota, &ajustes, cuando(4, 11)).expect("nota");
        assert_eq!(r.titulo, "Nota suelta para el diario");
        assert_eq!(r.fuente, Fuente::Nota);
        let notas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nota WHERE entrada_id = ?1 AND tipo='nota'",
                [r.id],
                |f| f.get(0),
            )
            .expect("notas");
        assert_eq!(notas, 1);
    }

    #[test]
    fn valida_url_y_titulo() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let ahora = cuando(4, 10);

        let mut sin_url = enlace("", None, None);
        sin_url.url = None;
        assert!(matches!(
            capturar(&conn, &sin_url, &ajustes, ahora),
            Err(Error::Validacion(_))
        ));
        assert!(capturar(&conn, &enlace("ftp://x.es/a", None, None), &ajustes, ahora).is_err());
        assert!(capturar(&conn, &enlace("no es url", None, None), &ajustes, ahora).is_err());
        let larga = format!("https://x.es/{}", "a".repeat(4096));
        assert!(capturar(&conn, &enlace(&larga, None, None), &ajustes, ahora).is_err());
        let titulo_largo = "t".repeat(1001);
        assert!(capturar(&conn, &texto("texto"), &ajustes, ahora).is_ok());
        let entrada = enlace("https://x.es/a", Some(&titulo_largo), None);
        assert!(capturar(&conn, &entrada, &ajustes, ahora).is_err());
    }

    #[test]
    fn recorta_seleccion_y_notas() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let largo = "á".repeat(25_000);
        let r = capturar(&conn, &texto(&largo), &ajustes, cuando(4, 10)).expect("captura");
        let seleccion: String = conn
            .query_row("SELECT seleccion FROM entrada WHERE id = ?1", [r.id], |f| {
                f.get(0)
            })
            .expect("selección");
        assert_eq!(seleccion.chars().count(), 20_000);
    }

    #[test]
    fn reindexar_deja_buscar_titulo_url_notas_y_extra() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        let mut captura = enlace(
            "https://empresa.atlassian.net/browse/PROJ-1896",
            Some("Listado de gestión"),
            None,
        );
        captura.nota = Some("pendiente de revisar con Ana".into());
        let r = capturar(&conn, &captura, &ajustes, cuando(4, 10)).expect("captura");

        for consulta in ["listado", "gestion", "jira", "proj", "ana", "browse"] {
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM entrada_fts WHERE entrada_fts MATCH ?1",
                    [format!("\"{consulta}\"*")],
                    |f| f.get(0),
                )
                .unwrap_or_else(|error| panic!("consulta {consulta}: {error}"));
            assert!(total >= 1, "no encuentra {consulta}");
        }
        // Reindexar dos veces no duplica.
        reindexar(&conn, r.id).expect("reindexa");
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM entrada_fts", [], |f| f.get(0))
            .expect("total");
        assert_eq!(total, 1);
    }
}
