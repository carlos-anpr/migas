//! Fuente GitLab: merge requests e issues/WI por su iid (`mr:ruta!39`, `issue:ruta#12`).

use super::{
    ErrorFuente, Identidad, Remoto, cadena, codificar, error_http, error_red, extracto, fecha,
    id_texto, opcional, token_gitlab, ultimos_comentarios,
};
use crate::config::Credenciales;
use crate::model::{Ajustes, Comentario, Huella};
use crate::{entidades, recurso};
use serde_json::Value;

/// Consulta un MR o una issue de GitLab y devuelve su huella.
pub async fn consultar(
    cli: &reqwest::Client,
    aj: &Ajustes,
    cred: &Credenciales,
    ident: &Identidad,
    clave: &str,
) -> Result<Remoto, ErrorFuente> {
    let (es_mr, proyecto, iid) = partes(clave)?;
    let yo = ident
        .usuario_gitlab(cli, aj, cred)
        .await
        .map_err(|error| error.contextual("GitLab", "al leer tu usuario"))?;
    let token = token_gitlab(cred)?;
    let base = format!("{}/api/v4", aj.gitlab_base_url.trim_end_matches('/'));
    let tipo = if es_mr { "merge_requests" } else { "issues" };
    let ruta = format!("{base}/projects/{}/{tipo}/{iid}", codificar(proyecto));

    let respuesta = cli
        .get(&ruta)
        .header("PRIVATE-TOKEN", token)
        .send()
        .await
        .map_err(|error| error_red("GitLab", &error))?;
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
        .map_err(|_| ErrorFuente::Red("Respuesta ilegible de GitLab".into()))?;

    let titulo = opcional(&cuerpo, &["title"]);
    let estado = opcional(&cuerpo, &["state"]);
    let descripcion = cadena(&cuerpo, &["description"]);
    let etiquetas = lista_de_texto(cuerpo.get("labels"));
    let asignado = asignados(cuerpo.get("assignees"));
    let actualizado = opcional(&cuerpo, &["updated_at"]).map(|texto| fecha(&texto));
    let pipeline = if es_mr {
        opcional(&cuerpo, &["head_pipeline", "status"])
    } else {
        None
    };
    let fusionada = es_mr && estado.as_deref() == Some("merged");

    let notas = leer_notas(cli, &ruta, token).await?;
    let mut comentarios: Vec<Comentario> = Vec::new();
    for (indice, valor) in notas.iter().enumerate() {
        if valor
            .get("system")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let autor = opcional(valor, &["author", "name"])
            .or_else(|| opcional(valor, &["author", "username"]))
            .unwrap_or_else(|| "Alguien".to_string());
        let mio = opcional(valor, &["author", "username"]).as_deref() == Some(yo.as_str());
        let mut id = id_texto(valor);
        if id.is_empty() {
            id = format!("gitlab-{indice}");
        }
        comentarios.push(Comentario {
            id,
            autor,
            mio,
            fecha: fecha(&cadena(valor, &["created_at"])),
            extracto: extracto(&cadena(valor, &["body"])),
        });
    }
    let total = cuerpo
        .get("user_notes_count")
        .and_then(Value::as_u64)
        .map_or(comentarios.len(), |total| total as usize);
    let ultimos = ultimos_comentarios(comentarios);

    let app = if es_mr {
        recurso::app_de_ruta(proyecto)
    } else if es_wi(proyecto) {
        entidades::app_de_wi(&descripcion)
    } else {
        recurso::app_de_ruta(proyecto)
    };
    let texto = format!("{}\n{descripcion}", titulo.clone().unwrap_or_default());
    let huella = Huella {
        titulo: titulo.clone(),
        estado: estado.clone(),
        asignado,
        etiquetas,
        pipeline,
        fusionada,
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
        app,
        identidad_error: None,
    })
}

/// `(es_mr, ruta del proyecto, iid)` a partir de la clave del recurso.
fn partes(clave: &str) -> Result<(bool, &str, u64), ErrorFuente> {
    let (es_mr, resto) = if let Some(resto) = clave.strip_prefix("mr:") {
        (true, resto)
    } else if let Some(resto) = clave.strip_prefix("issue:") {
        (false, resto)
    } else {
        return Err(ErrorFuente::Red("Recurso no válido".into()));
    };
    let (proyecto, numero) = resto
        .rsplit_once(if es_mr { '!' } else { '#' })
        .ok_or_else(|| ErrorFuente::Red("Recurso no válido".into()))?;
    if proyecto.is_empty() {
        return Err(ErrorFuente::Red("Recurso no válido".into()));
    }
    let iid = numero
        .parse()
        .map_err(|_| ErrorFuente::Red("Recurso no válido".into()))?;
    Ok((es_mr, proyecto, iid))
}

fn es_wi(proyecto: &str) -> bool {
    proyecto.eq_ignore_ascii_case("equipo/despliegues")
}

/// Páginas máximas de notas que se piden a GitLab.
const MAX_PAGINAS_NOTAS: u32 = 5;
/// Notas humanas que se quieren reunir (la ventana de la huella).
const NOTAS_HUMANAS: usize = 10;

async fn leer_notas(
    cli: &reqwest::Client,
    ruta: &str,
    token: &str,
) -> Result<Vec<Value>, ErrorFuente> {
    let mut notas = Vec::new();
    let mut pagina = 1u32;
    loop {
        let pagina_texto = pagina.to_string();
        let respuesta = cli
            .get(format!("{ruta}/notes"))
            .header("PRIVATE-TOKEN", token)
            .query(&[
                ("sort", "desc"),
                ("order_by", "created_at"),
                ("per_page", "20"),
                ("page", pagina_texto.as_str()),
            ])
            .send()
            .await
            .map_err(|error| error_red("GitLab", &error))?;
        match respuesta.status().as_u16() {
            // Un MR sin notas accesibles no debe tumbar la lectura del MR.
            404 => return Ok(notas),
            401 | 403 => return Err(error_http(respuesta.status())),
            _ => {}
        }
        if !respuesta.status().is_success() {
            return Err(error_http(respuesta.status()));
        }
        let siguiente = respuesta
            .headers()
            .get("x-next-page")
            .and_then(|valor| valor.to_str().ok())
            .and_then(|valor| valor.trim().parse::<u32>().ok())
            .filter(|siguiente| *siguiente > pagina);
        let cuerpo: Value = respuesta
            .json()
            .await
            .map_err(|_| ErrorFuente::Red("Respuesta ilegible de GitLab".into()))?;
        if let Some(lista) = cuerpo.as_array() {
            for valor in lista {
                if valor
                    .get("system")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    continue;
                }
                notas.push(valor.clone());
            }
        }
        if notas.len() >= NOTAS_HUMANAS || pagina >= MAX_PAGINAS_NOTAS || siguiente.is_none() {
            break;
        }
        pagina = siguiente.unwrap_or(pagina);
    }
    Ok(notas)
}

fn lista_de_texto(valor: Option<&Value>) -> Vec<String> {
    valor
        .and_then(Value::as_array)
        .map(|lista| {
            lista
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn asignados(valor: Option<&Value>) -> Option<String> {
    let nombres: Vec<String> = valor
        .and_then(Value::as_array)
        .map(|lista| {
            lista
                .iter()
                .filter_map(|asignado| {
                    opcional(asignado, &["name"]).or_else(|| opcional(asignado, &["username"]))
                })
                .collect()
        })
        .unwrap_or_default();
    if nombres.is_empty() {
        None
    } else {
        Some(nombres.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    fn ajustes(base: &str) -> Ajustes {
        Ajustes {
            gitlab_base_url: base.to_string(),
            ..Ajustes::default()
        }
    }

    fn credenciales() -> Credenciales {
        Credenciales {
            gitlab_token: Some("token-de-prueba".into()),
            ..Credenciales::default()
        }
    }

    fn ruta_mr() -> &'static str {
        "/api/v4/projects/grupo%2Fproy/merge_requests/39"
    }

    #[tokio::test]
    async fn lee_un_mr_con_notas_y_pipeline() {
        let servidor = MockServer::start();
        let yo = servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200).json_body(json!({"username": "yo"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path(ruta_mr());
            then.status(200).json_body(json!({
                "title": "Descarga de notas",
                "state": "opened",
                "assignees": [{"name": "Ana"}],
                "labels": ["front"],
                "head_pipeline": {"status": "running"},
                "updated_at": "2026-11-04T09:00:00Z",
                "user_notes_count": 3,
                "description": "Ver PROJ-12 y https://x/-/merge_requests/1"
            }));
        });
        let notas = servidor.mock(|when, then| {
            when.method(GET).path(format!("{}/notes", ruta_mr()));
            then.status(200).json_body(json!([
                {"id": 12, "body": "Hola\nmundo", "author": {"username": "ana", "name": "Ana"},
                 "created_at": "2026-11-04T08:00:00Z", "system": false},
                {"id": 11, "body": "Sistema", "author": {"username": "bot"},
                 "created_at": "2026-11-04T07:00:00Z", "system": true},
                {"id": 10, "body": "Mío", "author": {"username": "yo", "name": "Yo"},
                 "created_at": "2026-11-03T08:00:00Z", "system": false}
            ]));
        });

        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &Identidad::default(),
            "mr:grupo/proy!39",
        )
        .await
        .expect("MR");
        yo.assert();
        notas.assert();

        assert_eq!(remoto.titulo.as_deref(), Some("Descarga de notas"));
        assert_eq!(remoto.estado.as_deref(), Some("opened"));
        assert_eq!(remoto.huella.asignado.as_deref(), Some("Ana"));
        assert_eq!(remoto.huella.etiquetas, vec!["front"]);
        assert_eq!(remoto.huella.pipeline.as_deref(), Some("running"));
        assert!(!remoto.huella.fusionada);
        assert_eq!(remoto.huella.comentarios_total, 3);
        assert_eq!(
            remoto.huella.ultimos.len(),
            2,
            "las de sistema se descartan"
        );
        assert_eq!(remoto.huella.ultimos[0].id, "12");
        assert_eq!(remoto.huella.ultimos[0].extracto, "Hola mundo");
        assert_eq!(remoto.huella.ultimos[1].id, "10");
        assert!(remoto.huella.ultimos[1].mio);
        assert_eq!(remoto.app.as_deref(), Some("proy"));
    }

    #[tokio::test]
    async fn las_notas_de_sistema_paginan_hasta_las_humanas() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200).json_body(json!({"username": "yo"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path(ruta_mr());
            then.status(200).json_body(json!({
                "title": "Descarga de notas",
                "state": "opened",
                "updated_at": "2026-11-04T09:00:00Z",
                "user_notes_count": 1
            }));
        });
        let sistemas: Vec<serde_json::Value> = (0..20)
            .map(|indice| {
                json!({
                    "id": 100 + indice,
                    "body": "sistema",
                    "author": {"username": "bot"},
                    "created_at": "2026-11-04T08:00:00Z",
                    "system": true
                })
            })
            .collect();
        let primera = servidor.mock(|when, then| {
            when.method(GET)
                .path(format!("{}/notes", ruta_mr()))
                .query_param("page", "1");
            then.status(200)
                .header("x-next-page", "2")
                .json_body(serde_json::Value::Array(sistemas));
        });
        let segunda = servidor.mock(|when, then| {
            when.method(GET)
                .path(format!("{}/notes", ruta_mr()))
                .query_param("page", "2");
            then.status(200).json_body(json!([
                {"id": 7, "body": "Comentario humano", "author": {"username": "ana", "name": "Ana"},
                 "created_at": "2026-11-04T09:30:00Z", "system": false}
            ]));
        });

        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &Identidad::default(),
            "mr:grupo/proy!39",
        )
        .await
        .expect("MR");
        primera.assert();
        segunda.assert_calls(1);
        assert_eq!(remoto.huella.comentarios_total, 1);
        assert_eq!(remoto.huella.ultimos.len(), 1);
        assert_eq!(remoto.huella.ultimos[0].id, "7");
        assert_eq!(remoto.huella.ultimos[0].autor, "Ana");
    }

    #[tokio::test]
    async fn mr_fusionada() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200).json_body(json!({"username": "yo"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path(ruta_mr());
            then.status(200).json_body(json!({
                "title": "Ya está",
                "state": "merged",
                "updated_at": "2026-11-04T09:00:00Z"
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET).path(format!("{}/notes", ruta_mr()));
            then.status(200).json_body(json!([]));
        });
        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &Identidad::default(),
            "mr:grupo/proy!39",
        )
        .await
        .expect("MR");
        assert!(remoto.huella.fusionada);
        assert!(remoto.huella.pipeline.is_none());
    }

    #[tokio::test]
    async fn una_wi_aporta_su_app() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200).json_body(json!({"username": "yo"}));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v4/projects/equipo%2Fdespliegues/issues/1234");
            then.status(200).json_body(json!({
                "title": "Desplegar",
                "state": "opened",
                "description": "## Aplicación\n\ninformes-web (v1.2)",
                "updated_at": "2026-11-04T09:00:00Z"
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v4/projects/equipo%2Fdespliegues/issues/1234/notes");
            then.status(200).json_body(json!([]));
        });
        let remoto = consultar(
            &reqwest::Client::new(),
            &ajustes(&servidor.base_url()),
            &credenciales(),
            &Identidad::default(),
            "issue:equipo/despliegues#1234",
        )
        .await
        .expect("WI");
        assert_eq!(remoto.app.as_deref(), Some("informes-web"));
    }

    #[tokio::test]
    async fn errores_y_credenciales() {
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200).json_body(json!({"username": "yo"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path(ruta_mr());
            then.status(404).body("interno");
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v4/projects/grupo%2Fproy/merge_requests/401");
            then.status(401).body("interno");
        });
        let cli = reqwest::Client::new();
        let aj = ajustes(&servidor.base_url());
        let error = consultar(
            &cli,
            &aj,
            &credenciales(),
            &Identidad::default(),
            "mr:grupo/proy!404",
        )
        .await
        .expect_err("404");
        assert_eq!(error, ErrorFuente::NoExiste);

        let error = consultar(
            &cli,
            &aj,
            &credenciales(),
            &Identidad::default(),
            "mr:grupo/proy!401",
        )
        .await
        .expect_err("401");
        assert_eq!(error, ErrorFuente::Http(401));
        assert_eq!(error.resumen("GitLab"), "GitLab respondió 401");

        let error = consultar(
            &cli,
            &aj,
            &Credenciales::default(),
            &Identidad::default(),
            "mr:grupo/proy!39",
        )
        .await
        .expect_err("sin token");
        assert_eq!(error, ErrorFuente::SinCredenciales);
    }

    #[tokio::test]
    async fn la_identidad_se_cachea() {
        let servidor = MockServer::start();
        let yo = servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200).json_body(json!({"username": "yo"}));
        });
        for iid in [1, 2] {
            servidor.mock(|when, then| {
                when.method(GET)
                    .path(format!("/api/v4/projects/a%2Fb/merge_requests/{iid}"));
                then.status(200)
                    .json_body(json!({"title": "x", "state": "opened"}));
            });
            servidor.mock(|when, then| {
                when.method(GET)
                    .path(format!("/api/v4/projects/a%2Fb/merge_requests/{iid}/notes"));
                then.status(200).json_body(json!([]));
            });
        }
        let ident = Identidad::default();
        let cli = reqwest::Client::new();
        let aj = ajustes(&servidor.base_url());
        let cred = credenciales();
        consultar(&cli, &aj, &cred, &ident, "mr:a/b!1")
            .await
            .expect("1");
        consultar(&cli, &aj, &cred, &ident, "mr:a/b!2")
            .await
            .expect("2");
        yo.assert_calls(1);
    }
}
