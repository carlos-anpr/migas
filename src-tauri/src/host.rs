//! Host de mensajería nativa de Chrome: marcos de 4 bytes LE + JSON, en bucle hasta EOF.

use crate::config;
use crate::error::{Error, Resultado};
use crate::model::{Ajustes, NuevaEntrada, Origen, TipoEntrada};
use crate::{captura, consultas, db};
use chrono::{DateTime, Local};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use url::Url;

/// Tamaño máximo de un mensaje: 1 MiB.
pub const MAX_MENSAJE: usize = 1024 * 1024;
/// Tope de título del contrato.
const MAX_TITULO: usize = 1_000;
/// Tope de URL del contrato.
const MAX_URL: usize = 4_096;

/// Resultado de leer un marco.
#[derive(Debug)]
pub enum Lectura {
    /// EOF limpio.
    Fin,
    Mensaje(Value),
    /// El marco declara más de 1 MiB: hay que responder error y salir.
    Grande,
}

/// Lee un marco (u32 LE + JSON UTF-8). `Fin` en EOF limpio.
pub fn leer_mensaje<R: Read>(lector: &mut R) -> Resultado<Lectura> {
    let mut cabecera = [0u8; 4];
    match lector.read_exact(&mut cabecera) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(Lectura::Fin),
        Err(error) => return Err(Error::Io(error)),
    }
    let largo = u32::from_le_bytes(cabecera) as usize;
    if largo > MAX_MENSAJE {
        return Ok(Lectura::Grande);
    }
    let mut datos = vec![0u8; largo];
    lector.read_exact(&mut datos).map_err(Error::Io)?;
    let valor = serde_json::from_slice(&datos)?;
    Ok(Lectura::Mensaje(valor))
}

/// Escribe un marco (u32 LE + JSON UTF-8).
pub fn escribir_mensaje<W: Write>(escritor: &mut W, mensaje: &Value) -> Resultado<()> {
    let datos = serde_json::to_vec(mensaje)?;
    if datos.len() > MAX_MENSAJE {
        return Err(Error::Validacion("La respuesta es demasiado grande".into()));
    }
    let largo = u32::try_from(datos.len())
        .map_err(|_| Error::Validacion("La respuesta es demasiado grande".into()))?;
    escritor
        .write_all(&largo.to_le_bytes())
        .map_err(Error::Io)?;
    escritor.write_all(&datos).map_err(Error::Io)?;
    escritor.flush().map_err(Error::Io)?;
    Ok(())
}

/// Procesa una petición de la extensión y devuelve la respuesta.
pub fn procesar(
    conn: &Connection,
    ajustes: &Ajustes,
    mensaje: &Value,
    ahora: DateTime<Local>,
) -> Value {
    match despachar(conn, ajustes, mensaje, ahora) {
        Ok(respuesta) => respuesta,
        Err(error) => respuesta_error(&error),
    }
}

fn despachar(
    conn: &Connection,
    ajustes: &Ajustes,
    mensaje: &Value,
    ahora: DateTime<Local>,
) -> Resultado<Value> {
    if mensaje.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(Error::Validacion("Versión no válida".into()));
    }
    match mensaje.get("tipo").and_then(Value::as_str) {
        Some("capturar") => procesar_capturar(conn, ajustes, mensaje, ahora),
        Some("nota") => procesar_nota(conn, ajustes, mensaje, ahora),
        Some("visita") => procesar_visita(mensaje),
        Some("estado") => procesar_estado(conn, ajustes, mensaje, ahora),
        Some("abrir") => procesar_abrir(conn, mensaje),
        _ => Err(Error::Validacion("Petición no válida".into())),
    }
}

fn procesar_capturar(
    conn: &Connection,
    ajustes: &Ajustes,
    mensaje: &Value,
    ahora: DateTime<Local>,
) -> Resultado<Value> {
    let url = validar_url(mensaje.get("url").and_then(Value::as_str))?;
    let titulo = mensaje
        .get("titulo")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if titulo.chars().count() > MAX_TITULO {
        return Err(Error::Validacion("El título es demasiado largo".into()));
    }
    let nueva = NuevaEntrada {
        tipo: TipoEntrada::Enlace,
        origen: Origen::Navegador,
        url: Some(url),
        titulo: Some(titulo),
        seleccion: campo_texto(mensaje, "seleccion"),
        nota: campo_texto(mensaje, "nota"),
    };
    let resultado = captura::capturar(conn, &nueva, ajustes, ahora)?;
    Ok(json!({
        "ok": true,
        "id": resultado.id,
        "duplicada": resultado.duplicada,
        "titulo": resultado.titulo,
        "fuente": resultado.fuente,
        "clave": resultado.clave,
        "cambios": resultado.cambios,
        "relacionadas": resultado.relacionadas,
    }))
}

fn procesar_nota(
    conn: &Connection,
    ajustes: &Ajustes,
    mensaje: &Value,
    ahora: DateTime<Local>,
) -> Resultado<Value> {
    let id = mensaje
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::Validacion("Falta el id".into()))?;
    let texto = mensaje
        .get("texto")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|texto| !texto.is_empty())
        .ok_or_else(|| Error::Validacion("La nota está vacía".into()))?;
    consultas::crear_nota(conn, ajustes, id, texto, ahora)?;
    Ok(json!({"ok": true}))
}

/// Visitar la página ya no marca visto: los cambios se quedan hasta marcarlos a mano.
/// Se sigue aceptando el mensaje para no romper extensiones que aún lo envían.
fn procesar_visita(mensaje: &Value) -> Resultado<Value> {
    validar_url(mensaje.get("url").and_then(Value::as_str))?;
    Ok(json!({"ok": true, "marcada": false}))
}

fn procesar_estado(
    conn: &Connection,
    ajustes: &Ajustes,
    mensaje: &Value,
    ahora: DateTime<Local>,
) -> Resultado<Value> {
    let url = validar_url(mensaje.get("url").and_then(Value::as_str))?;
    let estado = consultas::estado_url(conn, &url, ajustes, ahora)?;
    Ok(json!({
        "ok": true,
        "guardadaHoy": estado.guardada_hoy,
        "id": estado.id,
        "veces": estado.veces,
        "cambios": estado.cambios,
        "relacionadas": estado.relacionadas,
    }))
}

fn procesar_abrir(conn: &Connection, mensaje: &Value) -> Resultado<Value> {
    let id = mensaje.get("id").and_then(Value::as_i64);
    if let Some(id) = id
        && !existe_entrada(conn, id)?
    {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    }
    lanzar_app(id)?;
    Ok(json!({"ok": true}))
}

/// Lanza `migas --mostrar <id>` (o `migas`) desacoplado, sin esperar.
fn lanzar_app(id: Option<i64>) -> Resultado<()> {
    let exe = std::env::current_exe().map_err(Error::Io)?;
    let mut comando = Command::new(exe);
    if let Some(id) = id {
        comando.arg("--mostrar").arg(id.to_string());
    }
    let mut hijo = comando
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::Io)?;
    // Recoger el hijo sin bloquear el bucle del host (evita zombis).
    std::thread::spawn(move || {
        let _ = hijo.wait();
    });
    Ok(())
}

fn existe_entrada(conn: &Connection, id: i64) -> Resultado<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM entrada WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Bucle del host: lee de stdin y responde por stdout hasta EOF. Logs solo a stderr.
pub fn ejecutar() -> i32 {
    let conn = match db::abrir_defecto() {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!("migas host: {error}");
            return 1;
        }
    };
    let ajustes = config::cargar_ajustes();
    let mut entrada = std::io::stdin().lock();
    let mut salida = std::io::stdout().lock();

    loop {
        match leer_mensaje(&mut entrada) {
            Ok(Lectura::Fin) => return 0,
            Ok(Lectura::Mensaje(mensaje)) => {
                let ahora = Local::now();
                let _ = db::meta_escribir(&conn, "ultima_extension", &ahora.to_rfc3339());
                let respuesta = procesar(&conn, &ajustes, &mensaje, ahora);
                if escribir_mensaje(&mut salida, &respuesta).is_err() {
                    return 0;
                }
            }
            Ok(Lectura::Grande) => {
                let _ = escribir_mensaje(
                    &mut salida,
                    &json!({"ok": false, "error": "Mensaje demasiado grande"}),
                );
                return 1;
            }
            Err(Error::Json(_)) => {
                let respuesta = json!({"ok": false, "error": "Mensaje no válido"});
                if escribir_mensaje(&mut salida, &respuesta).is_err() {
                    return 0;
                }
            }
            Err(error) => {
                let _ = escribir_mensaje(&mut salida, &respuesta_error(&error));
                return 1;
            }
        }
    }
}

fn campo_texto(mensaje: &Value, clave: &str) -> Option<String> {
    mensaje
        .get(clave)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|texto| !texto.is_empty())
        .map(str::to_string)
}

fn validar_url(valor: Option<&str>) -> Resultado<String> {
    let url = valor
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

fn respuesta_error(error: &Error) -> Value {
    json!({"ok": false, "error": error.to_string()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use chrono::TimeZone;
    use std::io::Cursor;

    fn ahora() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap()
    }

    fn bd(dir: &std::path::Path) -> Connection {
        db::abrir(&dir.join("migas.db")).expect("BD de prueba")
    }

    fn marco(valor: &Value) -> Vec<u8> {
        let datos = serde_json::to_vec(valor).expect("json");
        let mut salida = (datos.len() as u32).to_le_bytes().to_vec();
        salida.extend(datos);
        salida
    }

    #[test]
    fn marco_ida_y_vuelta() {
        let original = json!({"v": 1, "tipo": "estado", "url": "https://x.es/a"});
        let mut bytes = Vec::new();
        escribir_mensaje(&mut bytes, &original).expect("escribe");
        assert_eq!(&bytes[..4], &((bytes.len() - 4) as u32).to_le_bytes());
        let mut lector = Cursor::new(bytes);
        match leer_mensaje(&mut lector).expect("lee") {
            Lectura::Mensaje(valor) => assert_eq!(valor, original),
            otro => panic!("esperaba mensaje: {otro:?}"),
        }
        assert!(matches!(
            leer_mensaje(&mut lector).expect("fin"),
            Lectura::Fin
        ));
        // Y el mismo marco escrito a mano se vuelve a leer.
        let mut lector = Cursor::new(marco(&original));
        assert!(matches!(
            leer_mensaje(&mut lector).expect("lee"),
            Lectura::Mensaje(_)
        ));
    }

    #[test]
    fn mensaje_gigante() {
        let mut bytes = ((MAX_MENSAJE + 1) as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        let mut lector = Cursor::new(bytes);
        assert!(matches!(
            leer_mensaje(&mut lector).expect("lee"),
            Lectura::Grande
        ));
    }

    #[test]
    fn json_roto() {
        let mut bytes = 4u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"nojs");
        let mut lector = Cursor::new(bytes);
        assert!(matches!(leer_mensaje(&mut lector), Err(Error::Json(_))));
    }

    #[test]
    fn respuesta_gigante_no_se_escribe() {
        let gigante = json!({"texto": "x".repeat(MAX_MENSAJE + 1)});
        let mut salida = Vec::new();
        assert!(escribir_mensaje(&mut salida, &gigante).is_err());
        assert!(salida.is_empty());
    }

    #[test]
    fn cada_tipo_de_mensaje() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();

        let captura = procesar(
            &conn,
            &ajustes,
            &json!({
                "v": 1, "tipo": "capturar",
                "url": "https://empresa.atlassian.net/browse/PROJ-1",
                "titulo": "Una issue",
                "seleccion": "texto seleccionado"
            }),
            ahora(),
        );
        assert_eq!(captura["ok"], true);
        assert_eq!(captura["clave"], "PROJ-1");
        assert_eq!(captura["fuente"], "jira");
        let id = captura["id"].as_i64().expect("id");

        let nota = procesar(
            &conn,
            &ajustes,
            &json!({"v": 1, "tipo": "nota", "id": id, "texto": "una nota"}),
            ahora(),
        );
        assert_eq!(nota, json!({"ok": true}));

        let estado = procesar(
            &conn,
            &ajustes,
            &json!({"v": 1, "tipo": "estado", "url": "https://empresa.atlassian.net/browse/PROJ-1"}),
            ahora(),
        );
        assert_eq!(estado["ok"], true);
        assert_eq!(estado["guardadaHoy"], true);
        assert_eq!(estado["id"], id);
        assert_eq!(estado["veces"], 1);
        assert_eq!(estado["cambios"], 0);

        let visita = procesar(
            &conn,
            &ajustes,
            &json!({"v": 1, "tipo": "visita", "url": "https://empresa.atlassian.net/browse/PROJ-1"}),
            ahora(),
        );
        assert_eq!(visita, json!({"ok": true, "marcada": false}));

        let abrir = procesar(
            &conn,
            &ajustes,
            &json!({"v": 1, "tipo": "abrir", "id": id}),
            ahora(),
        );
        assert_eq!(abrir, json!({"ok": true}));
    }

    #[test]
    fn visita_a_un_recurso_no_guardado() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let respuesta = procesar(
            &conn,
            &Ajustes::default(),
            &json!({"v": 1, "tipo": "visita", "url": "https://empresa.atlassian.net/browse/PROJ-99"}),
            ahora(),
        );
        assert_eq!(respuesta, json!({"ok": true, "marcada": false}));
    }

    #[test]
    fn valida_todo() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let ajustes = Ajustes::default();
        for mensaje in [
            json!({"tipo": "estado"}),
            json!({"v": 2, "tipo": "estado"}),
            json!({"v": 1, "tipo": "desconocido"}),
            json!({"v": 1, "tipo": "capturar"}),
            json!({"v": 1, "tipo": "capturar", "url": "ftp://x.es/a"}),
            json!({"v": 1, "tipo": "capturar", "url": "https://x.es/a", "titulo": "t".repeat(1001)}),
            json!({"v": 1, "tipo": "nota", "id": 9999, "texto": "hola"}),
            json!({"v": 1, "tipo": "nota", "id": 1, "texto": "  "}),
            json!({"v": 1, "tipo": "estado", "url": "no es url"}),
            json!({"v": 1, "tipo": "abrir", "id": 9999}),
        ] {
            let respuesta = procesar(&conn, &ajustes, &mensaje, ahora());
            assert_eq!(respuesta["ok"], false, "para {mensaje}");
            assert!(
                respuesta["error"]
                    .as_str()
                    .is_some_and(|texto| !texto.is_empty()),
                "falta el error en {mensaje}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn abrir_no_deja_procesos_zombis() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        conn.execute(
            "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo)
             VALUES ('2026-11-04', '2026-11-04T10:00:00+01:00', '2026-11-04T10:00:00+01:00',
                     'texto', 'cli', 'texto', 'x')",
            [],
        )
        .expect("entrada");
        let id = conn.last_insert_rowid();
        let respuesta = procesar(
            &conn,
            &Ajustes::default(),
            &json!({"v": 1, "tipo": "abrir", "id": id}),
            ahora(),
        );
        assert_eq!(respuesta, json!({"ok": true}));

        // El hijo de prueba sale al rechazar `--mostrar`; sin recogerlo quedaría zombi.
        std::thread::sleep(std::time::Duration::from_millis(500));
        let zombis = procesos_zombis();
        assert!(zombis.is_empty(), "quedaron procesos zombis: {zombis:?}");
    }

    /// PIDs de hijos en estado zombi de cualquiera de los hilos del proceso.
    #[cfg(target_os = "linux")]
    fn procesos_zombis() -> Vec<u32> {
        let mut pids = Vec::new();
        let Ok(tareas) = std::fs::read_dir("/proc/self/task") else {
            return pids;
        };
        for tarea in tareas.flatten() {
            let Ok(texto) = std::fs::read_to_string(tarea.path().join("children")) else {
                continue;
            };
            for parte in texto.split_whitespace() {
                if let Ok(pid) = parte.parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
        pids.retain(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    let cierre = stat.rfind(')')?;
                    let estado = stat[cierre + 1..].split_whitespace().next()?;
                    Some(estado == "Z")
                })
                .unwrap_or(false)
        });
        pids
    }
}
