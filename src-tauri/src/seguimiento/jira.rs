//! Fuente Jira: una issue por su clave (`jira:PROJ-12`).

use super::{
    ErrorFuente, Identidad, Remoto, cadena, error_http, error_red, extracto, fecha, id_texto,
    opcional, texto_doc, ultimos_comentarios,
};
use crate::config::Credenciales;
use crate::model::{Ajustes, Comentario, Huella};
use serde_json::Value;

/// Campos que pide el seguimiento de una issue (los comentarios van por su endpoint).
const CAMPOS: &str = "summary,status,assignee,updated,description,labels";

/// Consulta una issue de Jira y devuelve su huella.
pub async fn consultar(
    cli: &reqwest::Client,
    aj: &Ajustes,
    cred: &Credenciales,
    ident: &Identidad,
    clave: &str,
) -> Result<Remoto, ErrorFuente> {
    let Some(clave_jira) = clave.strip_prefix("jira:") else {
        return Err(ErrorFuente::Red("Recurso no válido".into()));
    };
    if clave_jira.is_empty() {
        return Err(ErrorFuente::Red("Recurso no válido".into()));
    }
    let cuenta = ident
        .cuenta_jira(cli, aj, cred)
        .await
        .map_err(|error| error.contextual("Jira", "al leer tu cuenta"))?;
    let usuario = cred
        .jira_user
        .as_deref()
        .filter(|valor| !valor.is_empty())
        .ok_or(ErrorFuente::SinCredenciales)?;
    let token = cred
        .jira_token
        .as_deref()
        .filter(|valor| !valor.is_empty())
        .ok_or(ErrorFuente::SinCredenciales)?;
    let base = aj.jira_base_url.trim_end_matches('/');

    let respuesta = cli
        .get(format!("{base}/rest/api/3/issue/{clave_jira}"))
        .basic_auth(usuario, Some(token))
        .query(&[("fields", CAMPOS)])
        .send()
        .await
        .map_err(|error| error_red("Jira", &error))?;
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
        .map_err(|_| ErrorFuente::Red("Respuesta ilegible de Jira".into()))?;
    let campos = cuerpo
        .get("fields")
        .ok_or_else(|| ErrorFuente::Red("Respuesta ilegible de Jira".into()))?;

    let titulo = opcional(campos, &["summary"]);
    let estado = opcional(campos, &["status", "name"]);
    let asignado = opcional(campos, &["assignee", "displayName"])
        .or_else(|| opcional(campos, &["assignee", "accountId"]));
    let etiquetas: Vec<String> = campos
        .get("labels")
        .and_then(Value::as_array)
        .map(|lista| {
            lista
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let actualizado = opcional(campos, &["updated"]).map(|texto| fecha(&texto));
    let descripcion = texto_doc(campos.get("description"));

    let respuesta = cli
        .get(format!("{base}/rest/api/3/issue/{clave_jira}/comment"))
        .basic_auth(usuario, Some(token))
        .query(&[("orderBy", "-created"), ("maxResults", "10")])
        .send()
        .await
        .map_err(|error| error_red("Jira", &error))?;
    match respuesta.status().as_u16() {
        404 => return Err(ErrorFuente::NoExiste),
        401 | 403 => return Err(error_http(respuesta.status())),
        _ => {}
    }
    if !respuesta.status().is_success() {
        return Err(error_http(respuesta.status()));
    }
    let cuerpo_comentarios: Value = respuesta
        .json()
        .await
        .map_err(|_| ErrorFuente::Red("Respuesta ilegible de Jira".into()))?;
    let mut comentarios: Vec<Comentario> = Vec::new();
    if let Some(lista) = cuerpo_comentarios.get("comments").and_then(Value::as_array) {
        for (indice, valor) in lista.iter().enumerate() {
            let autor = opcional(valor, &["author", "displayName"])
                .or_else(|| opcional(valor, &["author", "accountId"]))
                .unwrap_or_else(|| "Alguien".to_string());
            let mio = opcional(valor, &["author", "accountId"]).as_deref() == Some(cuenta.as_str());
            let mut id = id_texto(valor);
            if id.is_empty() {
                id = format!("jira-{indice}");
            }
            comentarios.push(Comentario {
                id,
                autor,
                mio,
                fecha: fecha(&cadena(valor, &["created"])),
                extracto: extracto(&texto_doc(valor.get("body"))),
            });
        }
    }
    let total = cuerpo_comentarios
        .get("total")
        .and_then(Value::as_u64)
        .map_or(comentarios.len(), |total| total as usize);
    let ultimos = ultimos_comentarios(comentarios);

    let texto = format!("{}\n{descripcion}", titulo.clone().unwrap_or_default());
    let huella = Huella {
        titulo: titulo.clone(),
        estado: estado.clone(),
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
        estado,
        app: None,
        identidad_error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    fn ajustes(base: &str) -> Ajustes {
        Ajustes {
            jira_base_url: base.to_string(),
            ..Ajustes::default()
        }
    }

    fn credenciales() -> Credenciales {
        Credenciales {
            jira_user: Some("yo@ejemplo.es".into()),
            jira_token: Some("token-de-prueba".into()),
            ..Credenciales::default()
        }
    }

    fn identidad() -> Identidad {
        Identidad::default()
    }

    #[tokio::test]
    async fn lee_una_issue_completa() {
        let servidor = MockServer::start();
        let yo = servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/myself");
            then.status(200).json_body(json!({"accountId": "acc-1"}));
        });
        let issue = servidor.mock(|when, then| {
            when.method(GET)
                .path("/rest/api/3/issue/PROJ-12")
                .query_param(
                    "fields",
                    "summary,status,assignee,updated,description,labels",
                );
            then.status(200).json_body(json!({
                "key": "PROJ-12",
                "fields": {
                    "summary": "Listado de notas",
                    "status": {"name": "En curso"},
                    "assignee": {"displayName": "Ana"},
                    "updated": "2026-11-04T09:00:00.000+0100",
                    "labels": ["backend"],
                    "description": {"type": "doc", "version": 1, "content": [
                        {"type": "paragraph", "content": [{"type": "text", "text": "Ver PROJ-1"}]}
                    ]}
                }
            }));
        });
        let comentarios = servidor.mock(|when, then| {
            when.method(GET)
                .path("/rest/api/3/issue/PROJ-12/comment")
                .query_param("orderBy", "-created")
                .query_param("maxResults", "10");
            then.status(200).json_body(json!({
                "total": 2,
                "comments": [
                    {"id": "2", "author": {"displayName": "Yo", "accountId": "acc-1"},
                     "created": "2026-11-04T10:00:00.000+0100",
                     "body": {"type": "doc", "version": 1, "content": [
                         {"type": "paragraph", "content": [{"type": "text", "text": "Mío"}]}
                     ]}},
                    {"id": "1", "author": {"displayName": "Ana", "accountId": "acc-2"},
                     "created": "2026-11-03T10:00:00.000+0100",
                     "body": {"type": "doc", "version": 1, "content": [
                         {"type": "paragraph", "content": [{"type": "text", "text": "Primero"}]}
                     ]}}
                ]
            }));
        });

        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &identidad(),
            "jira:PROJ-12",
        )
        .await
        .expect("consulta");
        yo.assert();
        issue.assert();
        comentarios.assert();

        assert_eq!(remoto.titulo.as_deref(), Some("Listado de notas"));
        assert_eq!(remoto.estado.as_deref(), Some("En curso"));
        assert_eq!(remoto.huella.asignado.as_deref(), Some("Ana"));
        assert_eq!(remoto.huella.etiquetas, vec!["backend"]);
        assert_eq!(remoto.huella.comentarios_total, 2);
        assert_eq!(
            remoto.huella.actualizado.as_deref(),
            Some("2026-11-04T09:00:00+01:00")
        );
        assert!(remoto.texto.contains("Ver PROJ-1"));
        assert_eq!(remoto.huella.ultimos.len(), 2);
        assert_eq!(remoto.huella.ultimos[0].id, "2");
        assert!(remoto.huella.ultimos[0].mio);
        assert_eq!(remoto.huella.ultimos[1].autor, "Ana");
        assert!(remoto.app.is_none());
    }

    #[tokio::test]
    async fn la_identidad_se_cachea() {
        let servidor = MockServer::start();
        let yo = servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/myself");
            then.status(200).json_body(json!({"accountId": "acc-1"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-1");
            then.status(200)
                .json_body(json!({"fields": {"summary": "Una"}}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-2");
            then.status(200)
                .json_body(json!({"fields": {"summary": "Otra"}}));
        });
        for clave in ["PROJ-1", "PROJ-2"] {
            servidor.mock(|when, then| {
                when.method(GET)
                    .path(format!("/rest/api/3/issue/{clave}/comment"));
                then.status(200)
                    .json_body(json!({"total": 0, "comments": []}));
            });
        }
        let ident = identidad();
        let cli = reqwest::Client::new();
        let aj = ajustes(&servidor.base_url());
        let cred = credenciales();
        consultar(&cli, &aj, &cred, &ident, "jira:PROJ-1")
            .await
            .expect("primera");
        consultar(&cli, &aj, &cred, &ident, "jira:PROJ-2")
            .await
            .expect("segunda");
        yo.assert_calls(1);
    }

    #[tokio::test]
    async fn usa_el_endpoint_de_comentarios_y_no_el_embebido() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/myself");
            then.status(200).json_body(json!({"accountId": "acc-1"}));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/rest/api/3/issue/PROJ-12")
                .query_param("fields", "summary,status,assignee,updated,description,labels");
            then.status(200).json_body(json!({
                "fields": {
                    "summary": "Con comentarios paginados",
                    // Parcial a propósito: el cliente no debe usarlo.
                    "comment": {
                        "startAt": 0,
                        "maxResults": 1,
                        "total": 2,
                        "comments": [
                            {"id": "1", "author": {"displayName": "Ana", "accountId": "acc-2"},
                             "created": "2026-11-03T10:00:00.000+0100",
                             "body": {"type": "doc", "version": 1, "content": [
                                 {"type": "paragraph", "content": [{"type": "text", "text": "Viejo"}]}
                             ]}}
                        ]
                    }
                }
            }));
        });
        let comentarios = servidor.mock(|when, then| {
            when.method(GET)
                .path("/rest/api/3/issue/PROJ-12/comment")
                .query_param("orderBy", "-created")
                .query_param("maxResults", "10");
            then.status(200).json_body(json!({
                "total": 2,
                "comments": [
                    {"id": "2", "author": {"displayName": "Yo", "accountId": "acc-1"},
                     "created": "2026-11-04T10:00:00.000+0100",
                     "body": {"type": "doc", "version": 1, "content": [
                         {"type": "paragraph", "content": [{"type": "text", "text": "Nuevo"}]}
                     ]}},
                    {"id": "1", "author": {"displayName": "Ana", "accountId": "acc-2"},
                     "created": "2026-11-03T10:00:00.000+0100",
                     "body": {"type": "doc", "version": 1, "content": [
                         {"type": "paragraph", "content": [{"type": "text", "text": "Viejo"}]}
                     ]}}
                ]
            }));
        });

        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &identidad(),
            "jira:PROJ-12",
        )
        .await
        .expect("consulta");
        comentarios.assert();
        assert_eq!(remoto.huella.comentarios_total, 2);
        assert_eq!(remoto.huella.ultimos.len(), 2);
        assert_eq!(remoto.huella.ultimos[0].id, "2");
        assert!(remoto.huella.ultimos[0].mio);
        assert_eq!(remoto.huella.ultimos[1].id, "1");
        assert!(!remoto.huella.ultimos[1].mio);
    }

    #[tokio::test]
    async fn errores_resumidos() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/myself");
            then.status(200).json_body(json!({"accountId": "acc-1"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-404");
            then.status(404).body("detalle interno");
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-401");
            then.status(401).body("detalle interno");
        });
        let cli = reqwest::Client::new();
        let aj = ajustes(&servidor.base_url());
        let cred = credenciales();
        let ident = identidad();

        let error = consultar(&cli, &aj, &cred, &ident, "jira:PROJ-404")
            .await
            .expect_err("404");
        assert_eq!(error, ErrorFuente::NoExiste);
        assert_eq!(error.resumen("Jira"), "No existe o sin acceso");

        let error = consultar(&cli, &aj, &cred, &ident, "jira:PROJ-401")
            .await
            .expect_err("401");
        assert_eq!(error, ErrorFuente::Http(401));
        assert_eq!(error.resumen("Jira"), "Jira respondió 401");
        assert!(!error.resumen("Jira").contains("detalle"));
    }

    #[tokio::test]
    async fn sin_credenciales_no_hay_red() {
        let cli = reqwest::Client::new();
        let error = consultar(
            &cli,
            &ajustes("http://127.0.0.1:1"),
            &Credenciales::default(),
            &identidad(),
            "jira:PROJ-1",
        )
        .await
        .expect_err("sin credenciales");
        assert_eq!(error, ErrorFuente::SinCredenciales);
        assert_eq!(error.resumen("Jira"), "Sin credenciales de Jira");
    }
}
