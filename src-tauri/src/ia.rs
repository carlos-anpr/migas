//! Resumen del día con el LLM configurado (API compatible con OpenAI).

use crate::config::Credenciales;
use crate::error::{Error, Resultado};
use crate::model::{Ajustes, Cambio, EntradaVista, Filtro, ResumenDia, Vista};
use crate::{consultas, db};
use chrono::Local;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const MAX_ENTRADAS: u32 = 60;
const MAX_TITULO: usize = 160;
const MAX_SELECCION: usize = 200;
const MAX_NOTAS: usize = 300;
const MAX_CAMBIOS: usize = 400;
const TIMEOUT: Duration = Duration::from_secs(60);

const MENSAJE_SISTEMA: &str = r#"Eres un asistente que resume el trabajo de un día a partir de las migas guardadas por una persona. Escribe en español de España, en 3 a 7 viñetas agrupadas por tarea o aplicación. Cada viñeta debe empezar por la clave del recurso si existe (por ejemplo PROJ-123, !39, FS#45600 o #78). Usa únicamente la información del contexto: no inventes tareas, estados, relaciones ni resultados. No incluyas saludos, despedidas, firmas ni Markdown decorativo, y no menciones que eres una IA ni que el texto lo ha generado una."#;

/// Genera el resumen del día, lo guarda (upsert) y lo devuelve.
pub async fn resumir_dia(
    conn_ruta: &Path,
    cred: &Credenciales,
    aj: &Ajustes,
    dia: &str,
) -> Resultado<ResumenDia> {
    let clave = cred
        .ia_key
        .as_deref()
        .filter(|valor| !valor.is_empty())
        .ok_or_else(|| Error::Validacion("Activa la IA en Ajustes".to_string()))?;

    let contexto = {
        let conn = db::abrir(conn_ruta)?;
        contexto_dia(&conn, dia)?
    };
    let texto = llamar_llm(aj, clave, &contexto).await?;
    let generado = Local::now().to_rfc3339();
    let conn = db::abrir(conn_ruta)?;
    conn.execute(
        "INSERT INTO resumen (dia, texto, generado) VALUES (?1, ?2, ?3)
         ON CONFLICT(dia) DO UPDATE SET texto = excluded.texto, generado = excluded.generado",
        params![dia, texto, generado],
    )?;
    Ok(ResumenDia {
        dia: dia.to_string(),
        texto,
        generado,
    })
}

/// Lee el resumen guardado de un día, si lo hay.
pub fn leer_resumen(conn: &Connection, dia: &str) -> Resultado<Option<ResumenDia>> {
    conn.query_row(
        "SELECT texto, generado FROM resumen WHERE dia = ?1",
        [dia],
        |fila| {
            Ok(ResumenDia {
                dia: dia.to_string(),
                texto: fila.get(0)?,
                generado: fila.get(1)?,
            })
        },
    )
    .optional()
    .map_err(Error::from)
}

fn contexto_dia(conn: &Connection, dia: &str) -> Resultado<String> {
    let filtro = Filtro {
        vista: Some(Vista::Dia),
        dia: Some(dia.to_string()),
        limite: Some(MAX_ENTRADAS),
        ..Filtro::default()
    };
    let listado = consultas::listar(conn, &filtro, Local::now())?;
    if listado.entradas.is_empty() {
        return Ok("Sin migas ese día.".to_string());
    }
    let mut lineas = Vec::with_capacity(listado.entradas.len());
    for entrada in &listado.entradas {
        let notas = notas_resumidas(conn, entrada.id)?;
        lineas.push(linea_entrada(entrada, &notas));
    }
    Ok(lineas.join("\n"))
}

fn linea_entrada(entrada: &EntradaVista, notas: &str) -> String {
    let mut partes = Vec::new();
    partes.push(format!("{} · {}", entrada.hora, fuente_texto(entrada)));
    partes.push(format!(
        "título: {}",
        recortar(&una_linea(&entrada.titulo), MAX_TITULO)
    ));
    if let Some(clave) = entrada.clave.as_deref() {
        partes.push(format!("clave: {clave}"));
    }
    if let Some(estado) = entrada.estado.as_deref() {
        partes.push(format!("estado: {}", una_linea(estado)));
    }
    if let Some(app) = entrada.app.as_deref() {
        partes.push(format!("app: {}", una_linea(app)));
    }
    if !notas.is_empty() {
        partes.push(format!("notas: {notas}"));
    }
    if let Some(seleccion) = entrada.seleccion.as_deref() {
        partes.push(format!(
            "selección: {}",
            recortar(&una_linea(seleccion), MAX_SELECCION)
        ));
    }
    let cambios = cambios_texto(&entrada.cambios);
    if !cambios.is_empty() {
        partes.push(format!("cambios: {cambios}"));
    }
    format!("- {}", partes.join(" · "))
}

fn notas_resumidas(conn: &Connection, entrada_id: i64) -> Resultado<String> {
    let mut consulta = conn.prepare("SELECT texto FROM nota WHERE entrada_id = ?1 ORDER BY id")?;
    let textos = consulta
        .query_map([entrada_id], |fila| fila.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut total = String::new();
    for texto in textos {
        let limpio = una_linea(&texto);
        if limpio.is_empty() {
            continue;
        }
        let separador = if total.is_empty() { 0 } else { 3 };
        let restante = MAX_NOTAS.saturating_sub(total.chars().count() + separador);
        if restante == 0 {
            break;
        }
        if separador > 0 {
            total.push_str(" · ");
        }
        total.push_str(&recortar(&limpio, restante));
        if total.chars().count() >= MAX_NOTAS {
            break;
        }
    }
    Ok(total)
}

fn fuente_texto(entrada: &EntradaVista) -> &'static str {
    match entrada.fuente {
        crate::model::Fuente::Jira => "jira",
        crate::model::Fuente::Gitlab => "gitlab",
        crate::model::Fuente::Fs => "fs",
        crate::model::Fuente::Web => "web",
        crate::model::Fuente::Texto => "texto",
        crate::model::Fuente::Nota => "nota",
    }
}

fn cambios_texto(cambios: &[Cambio]) -> String {
    let partes: Vec<String> = cambios
        .iter()
        .map(|cambio| match cambio {
            Cambio::Comentarios { n, autores, .. } => {
                let autores = autores.join(", ");
                if autores.is_empty() {
                    format!("{n} comentarios")
                } else {
                    format!("{n} comentarios de {autores}")
                }
            }
            Cambio::Estado { antes, despues } => {
                format!(
                    "estado: {} → {}",
                    antes.as_deref().unwrap_or("—"),
                    despues.as_deref().unwrap_or("—")
                )
            }
            Cambio::Asignado { antes, despues } => {
                format!(
                    "asignado: {} → {}",
                    antes.as_deref().unwrap_or("—"),
                    despues.as_deref().unwrap_or("—")
                )
            }
            Cambio::Titulo { antes, despues } => {
                format!(
                    "título: {} → {}",
                    antes.as_deref().unwrap_or("—"),
                    despues.as_deref().unwrap_or("—")
                )
            }
            Cambio::Pipeline { antes, despues } => {
                format!(
                    "pipeline: {} → {}",
                    antes.as_deref().unwrap_or("—"),
                    despues.as_deref().unwrap_or("—")
                )
            }
            Cambio::Fusionada => "fusionada".to_string(),
            Cambio::Etiquetas { anadidas, quitadas } => {
                let mut partes = Vec::new();
                if !anadidas.is_empty() {
                    partes.push(format!("+{}", anadidas.join(", ")));
                }
                if !quitadas.is_empty() {
                    partes.push(format!("-{}", quitadas.join(", ")));
                }
                format!("etiquetas: {}", partes.join(" "))
            }
        })
        .collect();
    recortar(&una_linea(&partes.join(" · ")), MAX_CAMBIOS)
}

fn una_linea(texto: &str) -> String {
    texto.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn recortar(texto: &str, maximo: usize) -> String {
    texto.chars().take(maximo).collect()
}

async fn llamar_llm(aj: &Ajustes, clave: &str, contexto: &str) -> Resultado<String> {
    let (modelo, base_url) = crate::config::ia_efectiva(aj, |clave| std::env::var(clave).ok());
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let cuerpo = json!({
        "model": modelo,
        "messages": [
            {"role": "system", "content": MENSAJE_SISTEMA},
            {"role": "user", "content": formato_usuario(contexto)},
        ],
        "temperature": 0.2,
        "max_tokens": 600,
    });
    let cliente = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| Error::Validacion("No se pudo preparar la IA".to_string()))?;
    let respuesta = cliente
        .post(&url)
        .bearer_auth(clave)
        .json(&cuerpo)
        .send()
        .await;
    let respuesta = match respuesta {
        Ok(respuesta) => respuesta,
        Err(error) if error.is_timeout() => {
            return Err(Error::Validacion("Timeout de la IA".to_string()));
        }
        Err(_) => {
            return Err(Error::Validacion(
                "No se pudo conectar con la IA".to_string(),
            ));
        }
    };
    if !respuesta.status().is_success() {
        return Err(Error::Validacion(format!(
            "La IA respondió {}",
            respuesta.status().as_u16()
        )));
    }
    let cuerpo: Value = respuesta
        .json()
        .await
        .map_err(|_| Error::Validacion("La IA no devolvió un resumen".to_string()))?;
    let texto = cuerpo
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|texto| !texto.is_empty())
        .ok_or_else(|| Error::Validacion("La IA no devolvió un resumen".to_string()))?;
    Ok(texto.to_string())
}

fn formato_usuario(contexto: &str) -> String {
    format!("Migas del día:\n{contexto}\n\nEscribe solo las viñetas del resumen.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    fn base(ruta: &Path) -> Connection {
        db::abrir(ruta).expect("BD")
    }

    fn preparar(ruta: &Path) -> Connection {
        let conn = base(ruta);
        let cambios = serde_json::to_string(&vec![Cambio::Estado {
            antes: Some("En curso".into()),
            despues: Some("En pruebas".into()),
        }])
        .expect("cambios");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url, estado, app, cambios, seguir)
             VALUES ('jira:PROJ-1', 'jira', 'https://x/browse/PROJ-1',
                     'En pruebas', 'app-demo', ?1, 1)",
            [cambios],
        )
        .expect("recurso");
        conn.execute(
            "INSERT INTO entrada
                (dia, creada, actualizada, tipo, origen, fuente, titulo, seleccion, recurso)
             VALUES ('2026-11-04', '2026-11-04T10:00:00+01:00', '2026-11-04T10:00:00+01:00',
                     'enlace', 'navegador', 'jira', ?1, ?2, 'jira:PROJ-1')",
            params![
                "Un título de prueba que no debe superar el límite",
                "Una selección de prueba"
            ],
        )
        .expect("entrada");
        conn.execute(
            "INSERT INTO nota (entrada_id, creada, tipo, texto)
             VALUES (last_insert_rowid(), '2026-11-04T10:01:00+01:00', 'nota', ?1)",
            ["Una nota de prueba"],
        )
        .expect("nota");
        conn
    }

    fn ajustes(base_url: &str) -> Ajustes {
        Ajustes {
            ia: true,
            ia_modelo: "modelo-de-prueba".into(),
            ia_base_url: base_url.to_string(),
            ..Ajustes::default()
        }
    }

    fn credenciales() -> Credenciales {
        Credenciales {
            ia_key: Some("clave-ia-de-prueba".into()),
            ..Credenciales::default()
        }
    }

    #[tokio::test]
    async fn respuesta_correcta_se_guarda_y_se_devuelve() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        preparar(&ruta);
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/chat/completions");
                then.status(200).json_body(json!({
                    "choices": [{"message": {"content": "- PROJ-1: se probó el lector.\n- Se dejó listo para revisar."}}]
                }));
            })
            .await;

        let resumen = resumir_dia(
            &ruta,
            &credenciales(),
            &ajustes(&server.base_url()),
            "2026-11-04",
        )
        .await
        .expect("resumen");
        assert_eq!(resumen.dia, "2026-11-04");
        assert!(resumen.texto.contains("PROJ-1"));
        assert!(!resumen.generado.is_empty());
        mock.assert_async().await;

        let conn = base(&ruta);
        let (texto, generado): (String, String) = conn
            .query_row(
                "SELECT texto, generado FROM resumen WHERE dia = '2026-11-04'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("resumen guardado");
        assert_eq!(texto, resumen.texto);
        assert_eq!(generado, resumen.generado);
    }

    #[tokio::test]
    async fn respuesta_401_se_resume_sin_filtrar_la_clave() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        preparar(&ruta);
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/chat/completions");
                then.status(401);
            })
            .await;

        let error = resumir_dia(
            &ruta,
            &credenciales(),
            &ajustes(&server.base_url()),
            "2026-11-04",
        )
        .await
        .expect_err("la IA debe fallar");
        assert_eq!(error.to_string(), "La IA respondió 401");
        assert!(!error.to_string().contains("clave-ia-de-prueba"));
    }

    #[tokio::test]
    async fn cuerpo_raro_no_deja_resumen() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        preparar(&ruta);
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/chat/completions");
                then.status(200).json_body(json!({"choices": []}));
            })
            .await;

        let error = resumir_dia(
            &ruta,
            &credenciales(),
            &ajustes(&server.base_url()),
            "2026-11-04",
        )
        .await
        .expect_err("el cuerpo raro debe fallar");
        assert_eq!(error.to_string(), "La IA no devolvió un resumen");
        let conn = base(&ruta);
        let guardado: Option<String> = conn
            .query_row(
                "SELECT texto FROM resumen WHERE dia = '2026-11-04'",
                [],
                |fila| fila.get(0),
            )
            .optional()
            .expect("consulta");
        assert!(guardado.is_none());
    }

    #[test]
    fn contexto_respeta_los_limites() {
        assert_eq!(
            recortar(&"x".repeat(500), MAX_TITULO).chars().count(),
            MAX_TITULO
        );
        assert_eq!(
            recortar(&"x".repeat(500), MAX_SELECCION).chars().count(),
            MAX_SELECCION
        );
        assert_eq!(
            recortar(&"x".repeat(500), MAX_NOTAS).chars().count(),
            MAX_NOTAS
        );
    }
}
