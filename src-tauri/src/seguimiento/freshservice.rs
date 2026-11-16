//! Fuente Freshservice: un ticket por su id (`fs:45600`).

use super::{
    ErrorFuente, Identidad, Remoto, cadena, error_http, error_red, extracto, fecha, id_texto,
    opcional, ultimos_comentarios,
};
use crate::config::Credenciales;
use crate::model::{Ajustes, Comentario, Huella};
use serde_json::Value;

/// Consulta un ticket de Freshservice y devuelve su huella.
pub async fn consultar(
    cli: &reqwest::Client,
    aj: &Ajustes,
    cred: &Credenciales,
    ident: &Identidad,
    clave: &str,
) -> Result<Remoto, ErrorFuente> {
    let Some(id) = clave
        .strip_prefix("fs:")
        .and_then(|id| id.parse::<u64>().ok())
    else {
        return Err(ErrorFuente::Red("Recurso no válido".into()));
    };
    let api_key = cred
        .fs_key
        .as_deref()
        .filter(|valor| !valor.is_empty())
        .ok_or(ErrorFuente::SinCredenciales)?;
    let (yo, identidad_error) = match ident.agente_fs(cli, aj, cred).await {
        Ok(agente) => (Some(agente), None),
        Err(error) => {
            // La identidad no impide seguir el ticket: se consulta con `mio = false` y el
            // fallo se resume en el informe de la vuelta.
            let error = error.contextual("Freshservice", "al leer tu agente");
            (None, Some(error.resumen("Freshservice")))
        }
    };
    let base = aj.fs_base_url.trim_end_matches('/');

    let respuesta = cli
        .get(format!("{base}/api/v2/tickets/{id}"))
        .basic_auth(api_key, Some("X"))
        .send()
        .await
        .map_err(|error| error_red("Freshservice", &error))?;
    match respuesta.status().as_u16() {
        404 => return Err(ErrorFuente::NoExiste),
        401 | 403 => return Err(error_http(respuesta.status())),
        _ => {}
    }
    if !respuesta.status().is_success() {
        return Err(error_http(respuesta.status()));
    }
    let cuerpo: Value = respuesta
        .json()
        .await
        .map_err(|_| ErrorFuente::Red("Respuesta ilegible de Freshservice".into()))?;
    let datos = cuerpo.get("ticket").unwrap_or(&cuerpo);

    let titulo = opcional(datos, &["subject"]);
    let estado_numero = datos.get("status").and_then(Value::as_u64).unwrap_or(0);
    let estado = estado_texto(estado_numero);
    let actualizado = opcional(datos, &["updated_at"]).map(|texto| fecha(&texto));
    let asignado = datos
        .get("responder_id")
        .and_then(Value::as_u64)
        .map(|agente| format!("Agente {agente}"));
    let etiquetas: Vec<String> = datos
        .get("tags")
        .and_then(Value::as_array)
        .map(|lista| {
            lista
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let descripcion = opcional(datos, &["description_text"])
        .or_else(|| opcional(datos, &["description"]))
        .unwrap_or_default();
    let conversaciones = leer_conversaciones(cli, base, api_key, id).await?;

    let mut comentarios: Vec<Comentario> = Vec::new();
    for (indice, valor) in conversaciones.iter().enumerate() {
        let autor_id = valor.get("user_id").and_then(Value::as_u64);
        let mio = yo.is_some_and(|yo| autor_id == Some(yo));
        let incoming = valor
            .get("incoming")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let autor = if mio {
            "Tú".to_string()
        } else {
            opcional(valor, &["from_email"])
                .or_else(|| opcional(valor, &["user", "name"]))
                .unwrap_or_else(|| {
                    if incoming {
                        "Solicitante".to_string()
                    } else {
                        format!("Agente {}", autor_id.unwrap_or(0))
                    }
                })
        };
        let mut id = id_texto(valor);
        if id.is_empty() {
            id = format!("fs-{indice}");
        }
        comentarios.push(Comentario {
            id,
            autor,
            mio,
            fecha: fecha(&cadena(valor, &["created_at"])),
            extracto: extracto(&cadena(valor, &["body_text"])),
        });
    }
    let total = conversaciones.len();
    let ultimos = ultimos_comentarios(comentarios);

    let texto = format!("{}\n{descripcion}", titulo.clone().unwrap_or_default());
    let huella = Huella {
        titulo: titulo.clone(),
        estado: Some(estado.clone()),
        asignado,
        etiquetas,
        pipeline: None,
        fusionada: false,
        actualizado,
        comentarios_total: u32::try_from(total).unwrap_or(u32::MAX),
        ultimos,
        tomada: String::new(),
    };
    Ok(Remoto {
        huella,
        texto,
        titulo,
        estado: Some(estado),
        app: None,
        identidad_error,
    })
}

/// Conversaciones por página en Freshservice.
const CONVERSACIONES_POR_PAGINA: usize = 100;
/// Páginas máximas de conversaciones.
const MAX_PAGINAS_CONVERSACIONES: u32 = 3;

async fn leer_conversaciones(
    cli: &reqwest::Client,
    base: &str,
    api_key: &str,
    id: u64,
) -> Result<Vec<Value>, ErrorFuente> {
    let mut todas = Vec::new();
    let mut pagina = 1u32;
    loop {
        let pagina_texto = pagina.to_string();
        let respuesta = cli
            .get(format!("{base}/api/v2/tickets/{id}/conversations"))
            .query(&[("per_page", "100"), ("page", pagina_texto.as_str())])
            .basic_auth(api_key, Some("X"))
            .send()
            .await
            .map_err(|error| error_red("Freshservice", &error))?;
        match respuesta.status().as_u16() {
            404 => break,
            401 | 403 => return Err(error_http(respuesta.status())),
            _ => {}
        }
        if !respuesta.status().is_success() {
            return Err(error_http(respuesta.status()));
        }
        let cuerpo: Value = respuesta
            .json()
            .await
            .map_err(|_| ErrorFuente::Red("Respuesta ilegible de Freshservice".into()))?;
        let lista = match &cuerpo {
            Value::Array(lista) => lista.clone(),
            Value::Object(_) => cuerpo
                .get("conversations")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let cuantas = lista.len();
        todas.extend(lista);
        if cuantas < CONVERSACIONES_POR_PAGINA || pagina >= MAX_PAGINAS_CONVERSACIONES {
            break;
        }
        pagina += 1;
    }
    Ok(todas)
}

/// 2 Abierto · 3 Pendiente · 4 Resuelto · 5 Cerrado · otro → `Estado N`.
fn estado_texto(estado: u64) -> String {
    match estado {
        2 => "Abierto".to_string(),
        3 => "Pendiente".to_string(),
        4 => "Resuelto".to_string(),
        5 => "Cerrado".to_string(),
        otro => format!("Estado {otro}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    fn ajustes(base: &str) -> Ajustes {
        Ajustes {
            fs_base_url: base.to_string(),
            ..Ajustes::default()
        }
    }

    fn credenciales() -> Credenciales {
        Credenciales {
            fs_key: Some("clave-de-prueba".into()),
            ..Credenciales::default()
        }
    }

    fn mocks(servidor: &MockServer) {
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/agents/me");
            then.status(200).json_body(json!({"agent": {"id": 7}}));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/5")
                .query_param_missing("include");
            then.status(200).json_body(json!({
                "ticket": {
                    "id": 5,
                    "subject": "Descarga de notas",
                    "status": 4,
                    "updated_at": "2026-11-04T09:00:00Z",
                    "responder_id": 7,
                    "description_text": "Ver PROJ-12"
                }
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/5/conversations")
                .query_param("per_page", "100")
                .query_param("page", "1");
            then.status(200).json_body(json!([
                {"id": 1, "user_id": 9, "body_text": "Hola\nmundo",
                 "created_at": "2026-11-03T10:00:00Z", "incoming": true},
                {"id": 2, "user_id": 7, "body_text": "Mío",
                 "created_at": "2026-11-04T10:00:00Z", "incoming": false}
            ]));
        });
    }

    #[tokio::test]
    async fn lee_un_ticket_con_conversaciones() {
        let servidor = MockServer::start();
        mocks(&servidor);
        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &Identidad::default(),
            "fs:5",
        )
        .await
        .expect("ticket");
        assert_eq!(remoto.titulo.as_deref(), Some("Descarga de notas"));
        assert_eq!(remoto.estado.as_deref(), Some("Resuelto"));
        assert_eq!(remoto.huella.asignado.as_deref(), Some("Agente 7"));
        assert_eq!(remoto.huella.comentarios_total, 2);
        assert_eq!(remoto.huella.ultimos.len(), 2);
        assert_eq!(remoto.huella.ultimos[0].id, "2");
        assert!(remoto.huella.ultimos[0].mio);
        assert_eq!(remoto.huella.ultimos[0].autor, "Tú");
        assert_eq!(remoto.huella.ultimos[1].autor, "Solicitante");
        assert_eq!(remoto.huella.ultimos[1].extracto, "Hola mundo");
        assert!(remoto.texto.contains("Ver PROJ-12"));
    }

    #[tokio::test]
    async fn pagina_las_conversaciones_de_cien_en_cien() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/agents/me");
            then.status(200).json_body(json!({"agent": {"id": 7}}));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/9")
                .query_param_missing("include");
            then.status(200).json_body(json!({
                "ticket": {
                    "id": 9,
                    "subject": "Ticket largo",
                    "status": 2,
                    "updated_at": "2026-11-04T10:00:00Z"
                }
            }));
        });
        let muchas: Vec<serde_json::Value> = (0..100)
            .map(|indice| {
                json!({
                    "id": 1000 + indice,
                    "user_id": 9,
                    "body_text": format!("Conversación {indice}"),
                    "created_at": "2026-11-03T10:00:00Z",
                    "incoming": true
                })
            })
            .collect();
        let primera = servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/9/conversations")
                .query_param("per_page", "100")
                .query_param("page", "1");
            then.status(200).json_body(serde_json::Value::Array(muchas));
        });
        let segunda = servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/9/conversations")
                .query_param("per_page", "100")
                .query_param("page", "2");
            then.status(200).json_body(json!([
                {"id": 2000, "user_id": 7, "body_text": "La más nueva",
                 "created_at": "2026-11-04T10:00:00Z", "incoming": false}
            ]));
        });

        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &Identidad::default(),
            "fs:9",
        )
        .await
        .expect("ticket");
        primera.assert();
        segunda.assert_calls(1);
        assert_eq!(remoto.huella.comentarios_total, 101);
        assert_eq!(remoto.huella.ultimos.len(), 10);
        assert_eq!(remoto.huella.ultimos[0].id, "2000");
        assert!(remoto.huella.ultimos[0].mio);
    }

    #[test]
    fn estados() {
        assert_eq!(estado_texto(2), "Abierto");
        assert_eq!(estado_texto(3), "Pendiente");
        assert_eq!(estado_texto(5), "Cerrado");
        assert_eq!(estado_texto(7), "Estado 7");
    }

    #[tokio::test]
    async fn la_identidad_se_cachea() {
        let servidor = MockServer::start();
        let agentes = servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/agents/me");
            then.status(200).json_body(json!({"agent": {"id": 7}}));
        });
        for id in [5, 6] {
            servidor.mock(|when, then| {
                when.method(GET)
                    .path(format!("/api/v2/tickets/{id}"))
                    .query_param_missing("include");
                then.status(200).json_body(json!({
                    "ticket": {
                        "id": id,
                        "subject": "Ticket",
                        "status": 2
                    }
                }));
            });
            servidor.mock(|when, then| {
                when.method(GET)
                    .path(format!("/api/v2/tickets/{id}/conversations"));
                then.status(200).json_body(json!([]));
            });
        }
        let ident = Identidad::default();
        let cli = reqwest::Client::new();
        let aj = ajustes(&servidor.base_url());
        let cred = credenciales();
        consultar(&cli, &aj, &cred, &ident, "fs:5")
            .await
            .expect("5");
        consultar(&cli, &aj, &cred, &ident, "fs:6")
            .await
            .expect("6");
        agentes.assert_calls(1);
    }

    #[tokio::test]
    async fn errores_y_credenciales() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/agents/me");
            then.status(200).json_body(json!({"agent": {"id": 7}}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/tickets/404");
            then.status(404).body("interno");
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/tickets/401");
            then.status(401).body("interno");
        });
        let cli = reqwest::Client::new();
        let aj = ajustes(&servidor.base_url());
        let cred = credenciales();
        let ident = Identidad::default();

        let error = consultar(&cli, &aj, &cred, &ident, "fs:404")
            .await
            .expect_err("404");
        assert_eq!(error, ErrorFuente::NoExiste);
        let error = consultar(&cli, &aj, &cred, &ident, "fs:401")
            .await
            .expect_err("401");
        assert_eq!(error, ErrorFuente::Http(401));

        let error = consultar(
            &cli,
            &aj,
            &Credenciales::default(),
            &Identidad::default(),
            "fs:5",
        )
        .await
        .expect_err("sin clave");
        assert_eq!(error, ErrorFuente::SinCredenciales);
        assert_eq!(
            error.resumen("Freshservice"),
            "Sin credenciales de Freshservice"
        );
    }
}
