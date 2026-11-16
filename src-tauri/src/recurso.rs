//! URL → recurso reconocido (Jira, GitLab, Freshservice) y normalización de URLs.

use crate::error::Resultado;
use crate::model::{Ajustes, Fuente, Recurso, TipoRecurso};
use chrono::{DateTime, Local};
use regex::Regex;
use rusqlite::params;
use std::sync::{LazyLock, RwLock};
use url::Url;

/// `PROJ-1896`: prefijo de 2 a 10 caracteres y número de hasta 6 cifras.
/// Nombre del repositorio de work items (ajuste `repoWi`). Se fija al arrancar y al guardar
/// los ajustes; vacío = sin WI.
static REPO_WI: RwLock<String> = RwLock::new(String::new());

/// Fija el repositorio de work items (último segmento de la ruta, sin `.git`).
pub fn fijar_repo_wi(nombre: &str) {
    let nombre = nombre.trim().trim_end_matches('/');
    let nombre = nombre.rsplit('/').next().unwrap_or("");
    let nombre = nombre.strip_suffix(".git").unwrap_or(nombre);
    if let Ok(mut guardia) = REPO_WI.write() {
        *guardia = nombre.to_string();
    }
}

/// Repositorio de work items en vigor. En las pruebas, `despliegues` si no se ha fijado.
pub fn repo_wi() -> String {
    let valor = REPO_WI.read().map(|g| g.clone()).unwrap_or_default();
    if cfg!(test) && valor.is_empty() {
        return "despliegues".to_string();
    }
    valor
}

static RE_CLAVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([a-z][a-z0-9]{1,9})-(\d{1,6})$").expect("regex de clave Jira")
});

/// URL normalizada: sin fragmento, sin parámetros de ruido, sin `/` final y host en minúsculas.
pub fn normalizar(url: &str) -> String {
    let recortada = url.trim();
    let Ok(mut u) = Url::parse(recortada) else {
        return recortada.to_string();
    };
    u.set_fragment(None);
    if let Some(consulta) = u.query() {
        let partes: Vec<&str> = consulta.split('&').collect();
        let conservadas: Vec<&str> = partes
            .iter()
            .copied()
            .filter(|parte| !es_parametro_ruido(parte.split('=').next().unwrap_or("")))
            .collect();
        if conservadas.is_empty() {
            u.set_query(None);
        } else if conservadas.len() != partes.len() {
            u.set_query(Some(&conservadas.join("&")));
        }
    }
    let camino = u.path().to_string();
    if camino.len() > 1 && camino.ends_with('/') {
        let sin = camino.trim_end_matches('/');
        u.set_path(if sin.is_empty() { "/" } else { sin });
    }
    if let Some(host) = u.host_str().map(str::to_string) {
        let minusculas = host.to_lowercase();
        if minusculas != host {
            let _ = u.set_host(Some(&minusculas));
        }
    }
    u.to_string()
}

/// Parámetros que no aportan nada al enlace (`utm_*`, `fbclid`, `gclid`, foco de comentario).
fn es_parametro_ruido(clave: &str) -> bool {
    let clave = clave.to_ascii_lowercase();
    clave.starts_with("utm_")
        || matches!(
            clave.as_str(),
            "fbclid" | "gclid" | "focusedcommentid" | "focusedid"
        )
}

/// Reconoce un recurso seguible en una URL. `None` si no lo es.
pub fn reconocer(url: &str, ajustes: &Ajustes) -> Option<Recurso> {
    let limpia = normalizar(url);
    let u = Url::parse(&limpia).ok()?;
    if mismo_host(&u, &ajustes.jira_base_url) {
        return reconocer_jira(&u, ajustes);
    }
    if mismo_host(&u, &ajustes.gitlab_base_url) {
        return reconocer_gitlab(&u, ajustes);
    }
    if mismo_host(&u, &ajustes.fs_base_url) {
        return reconocer_fs(&u, ajustes);
    }
    None
}

fn reconocer_jira(u: &Url, ajustes: &Ajustes) -> Option<Recurso> {
    let segmentos: Vec<&str> = u.path_segments()?.collect();
    let clave = if let Some(ultimo) = segmentos.last()
        && segmentos.len() >= 2
        && segmentos[segmentos.len() - 2].eq_ignore_ascii_case("browse")
    {
        clave_jira(ultimo)
    } else if let Some(clave) = u
        .query_pairs()
        .find(|(nombre, _)| nombre.eq_ignore_ascii_case("selectedIssue"))
        .and_then(|(_, valor)| clave_jira(&valor))
    {
        Some(clave)
    } else {
        segmentos.last().and_then(|ultimo| clave_jira(ultimo))
    }?;
    let (prefijo, _) = clave.split_once('-')?;
    Some(Recurso {
        tipo: TipoRecurso::Jira,
        clave: format!("jira:{clave}"),
        url: format!("{}/browse/{clave}", base_limpia(&ajustes.jira_base_url)),
        proyecto: Some(prefijo.to_string()),
        visible: clave,
        es_wi: false,
    })
}

fn reconocer_gitlab(u: &Url, ajustes: &Ajustes) -> Option<Recurso> {
    let ruta = u.path().trim_start_matches('/');
    let (proyecto, resto) = ruta.split_once("/-/")?;
    let proyecto = proyecto.trim_matches('/');
    if proyecto.is_empty() {
        return None;
    }
    let segmentos: Vec<&str> = resto.split('/').collect();
    let tipo_pagina = *segmentos.first()?;
    let numero: u64 = segmentos.get(1)?.parse().ok()?;
    let base = base_limpia(&ajustes.gitlab_base_url);
    let es_wi = es_repo_wi(proyecto);
    match tipo_pagina {
        "merge_requests" => Some(Recurso {
            tipo: TipoRecurso::Mr,
            clave: format!("mr:{proyecto}!{numero}"),
            url: format!("{base}/{proyecto}/-/merge_requests/{numero}"),
            proyecto: Some(proyecto.to_string()),
            visible: format!("!{numero}"),
            es_wi: false,
        }),
        "issues" | "work_items" => Some(Recurso {
            tipo: TipoRecurso::Issue,
            clave: format!("issue:{proyecto}#{numero}"),
            // Identidad única: las dos rutas de GitLab son la misma issue/WI.
            url: format!("{base}/{proyecto}/-/issues/{numero}"),
            proyecto: Some(proyecto.to_string()),
            visible: if es_wi {
                format!("WI {numero}")
            } else {
                format!("#{numero}")
            },
            es_wi,
        }),
        _ => None,
    }
}

fn reconocer_fs(u: &Url, ajustes: &Ajustes) -> Option<Recurso> {
    let segmentos: Vec<&str> = u.path_segments()?.collect();
    let numero: u64 = match segmentos.as_slice() {
        ["a", "tickets", numero] | ["helpdesk", "tickets", numero] => numero.parse().ok()?,
        _ => return None,
    };
    Some(Recurso {
        tipo: TipoRecurso::Fs,
        clave: format!("fs:{numero}"),
        url: format!("{}/a/tickets/{numero}", base_limpia(&ajustes.fs_base_url)),
        proyecto: None,
        visible: format!("FS#{numero}"),
        es_wi: false,
    })
}

/// Ruta de proyecto de cualquier página GitLab con `/-/` (`grupo/proy`).
pub fn proyecto_gitlab(url: &str, ajustes: &Ajustes) -> Option<String> {
    let u = Url::parse(url.trim()).ok()?;
    if !mismo_host(&u, &ajustes.gitlab_base_url) {
        return None;
    }
    proyecto_de_ruta(u.path())
}

/// Ruta de proyecto de un path de GitLab con `/-/`, sin mirar el host.
pub(crate) fn proyecto_de_ruta(path: &str) -> Option<String> {
    let ruta = path.trim_start_matches('/');
    let (proyecto, _) = ruta.split_once("/-/")?;
    let proyecto = proyecto.trim_matches('/');
    if proyecto.is_empty() {
        None
    } else {
        Some(proyecto.to_string())
    }
}

/// Fuente visual de una URL: `jira`, `gitlab`, `fs` si es de ese host (aunque no sea recurso);
/// `web` para cualquier otra.
pub fn fuente_de_url(url: &str, ajustes: &Ajustes) -> Fuente {
    let Ok(u) = Url::parse(url.trim()) else {
        return Fuente::Web;
    };
    if mismo_host(&u, &ajustes.jira_base_url) {
        Fuente::Jira
    } else if mismo_host(&u, &ajustes.gitlab_base_url) {
        Fuente::Gitlab
    } else if mismo_host(&u, &ajustes.fs_base_url) {
        Fuente::Fs
    } else {
        Fuente::Web
    }
}

/// Texto visible de una clave: `jira:PROJ-1` → `PROJ-1`, `mr:a/b!3` → `!3`,
/// `issue:a/b#9` → `#9`, `issue:…/<repoWi>#9` → `WI 9`, `fs:5` → `FS#5`.
pub fn visible(clave: &str) -> String {
    let (tipo, resto) = clave.split_once(':').unwrap_or(("", clave));
    match tipo {
        "jira" => resto.to_string(),
        "fs" => format!("FS#{resto}"),
        "wi" => format!("WI {resto}"),
        "mr" => match resto.rsplit_once('!') {
            Some((_, numero)) => format!("!{numero}"),
            None => resto.to_string(),
        },
        "issue" => match resto.rsplit_once('#') {
            Some((proyecto, numero)) => {
                if es_repo_wi(proyecto) {
                    format!("WI {numero}")
                } else {
                    format!("#{numero}")
                }
            }
            None => resto.to_string(),
        },
        _ => clave.to_string(),
    }
}

/// `true` si la ruta es el repositorio de work items de despliegues. Ese repo contiene
/// las WI de todas las apps, así que no puede ser proyecto ni app de una entrada.
pub fn es_repo_wi(ruta: &str) -> bool {
    let ultimo = ruta.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let ultimo = ultimo.strip_suffix(".git").unwrap_or(ultimo);
    let repo = repo_wi();
    !repo.is_empty() && ultimo.eq_ignore_ascii_case(&repo)
}

/// `true` si la entidad `proyecto:`/`app:` no debe existir para ese repositorio.
pub fn es_entidad_prohibida(valor: &str) -> bool {
    if let Some(ruta) = valor.strip_prefix("proyecto:") {
        es_repo_wi(ruta)
    } else if let Some(app) = valor.strip_prefix("app:") {
        es_repo_wi(app)
    } else {
        false
    }
}

/// App a partir de la ruta de un proyecto GitLab: último segmento en minúsculas.
/// El repositorio de work items no es una app.
pub fn app_de_ruta(ruta: &str) -> Option<String> {
    if es_repo_wi(ruta) {
        return None;
    }
    let ultimo = ruta.trim_end_matches('/').rsplit('/').next()?;
    let ultimo = ultimo.strip_suffix(".git").unwrap_or(ultimo);
    if ultimo.is_empty() {
        return None;
    }
    Some(ultimo.to_lowercase())
}

/// Marca un recurso como visto: `huella_vista = huella_actual`, sin cambios.
/// Si aún no hay huella, deja `pendiente_visto = 1` para copiarla cuando llegue.
pub fn marcar_visto(
    conn: &rusqlite::Connection,
    clave: &str,
    ahora: DateTime<Local>,
) -> Resultado<()> {
    conn.execute(
        "UPDATE recurso
            SET huella_vista = huella_actual,
                cambios = NULL,
                visto = ?2,
                pendiente_visto = CASE WHEN huella_actual IS NULL THEN 1 ELSE 0 END
          WHERE clave = ?1",
        params![clave, ahora.to_rfc3339()],
    )?;
    Ok(())
}

/// Como `marcar_visto`, pero solo si el recurso nunca se ha visto: fija la base al
/// capturarlo por primera vez sin borrar cambios pendientes de revisar.
pub fn marcar_visto_si_nuevo(
    conn: &rusqlite::Connection,
    clave: &str,
    ahora: DateTime<Local>,
) -> Resultado<()> {
    conn.execute(
        "UPDATE recurso
            SET huella_vista = huella_actual,
                visto = ?2,
                pendiente_visto = CASE WHEN huella_actual IS NULL THEN 1 ELSE 0 END
          WHERE clave = ?1 AND visto IS NULL AND huella_vista IS NULL AND cambios IS NULL",
        params![clave, ahora.to_rfc3339()],
    )?;
    Ok(())
}

fn base_limpia(base: &str) -> &str {
    base.trim().trim_end_matches('/')
}

fn mismo_host(u: &Url, base: &str) -> bool {
    let Ok(base) = Url::parse(base.trim()) else {
        return false;
    };
    match (u.host_str(), base.host_str()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

fn clave_jira(valor: &str) -> Option<String> {
    let capturas = RE_CLAVE.captures(valor.trim())?;
    let prefijo = capturas.get(1)?.as_str().to_uppercase();
    let numero: u64 = capturas.get(2)?.as_str().parse().ok()?;
    Some(format!("{prefijo}-{numero}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use chrono::TimeZone;

    fn ajustes() -> Ajustes {
        Ajustes::default()
    }

    fn reconocido(url: &str) -> Recurso {
        reconocer(url, &ajustes()).expect("la URL debería ser un recurso")
    }

    #[test]
    fn reconoce_las_urls_de_la_tabla() {
        let jira = reconocido("https://empresa.atlassian.net/browse/PROJ-1896");
        assert_eq!(jira.tipo, TipoRecurso::Jira);
        assert_eq!(jira.clave, "jira:PROJ-1896");
        assert_eq!(jira.url, "https://empresa.atlassian.net/browse/PROJ-1896");
        assert_eq!(jira.proyecto.as_deref(), Some("PROJ"));
        assert_eq!(jira.visible, "PROJ-1896");
        assert!(!jira.es_wi);

        let tablero = reconocido(
            "https://empresa.atlassian.net/jira/software/projects/PROJ/boards/12?selectedIssue=PROJ-1896&foo=1",
        );
        assert_eq!(tablero.clave, "jira:PROJ-1896");
        assert_eq!(
            tablero.url,
            "https://empresa.atlassian.net/browse/PROJ-1896"
        );

        let issues = reconocido(
            "https://empresa.atlassian.net/jira/software/c/projects/PROJ/issues/PROJ-1896",
        );
        assert_eq!(issues.clave, "jira:PROJ-1896");

        let cola = reconocido(
            "https://empresa.atlassian.net/jira/software/c/projects/PROJ/queues/issue/PROJ-1896",
        );
        assert_eq!(cola.clave, "jira:PROJ-1896");

        let mr = reconocido("https://gitlab.com/equipo/informes-web/-/merge_requests/39");
        assert_eq!(mr.tipo, TipoRecurso::Mr);
        assert_eq!(mr.clave, "mr:equipo/informes-web!39");
        assert_eq!(mr.visible, "!39");
        assert_eq!(mr.proyecto.as_deref(), Some("equipo/informes-web"));
        assert_eq!(
            mr.url,
            "https://gitlab.com/equipo/informes-web/-/merge_requests/39"
        );

        let issue = reconocido("https://gitlab.com/a/b/-/issues/1234");
        assert_eq!(issue.tipo, TipoRecurso::Issue);
        assert_eq!(issue.clave, "issue:a/b#1234");
        assert_eq!(issue.visible, "#1234");
        assert!(!issue.es_wi);

        let wi = reconocido("https://gitlab.com/equipo/despliegues/-/issues/1234");
        assert!(wi.es_wi);
        assert_eq!(wi.clave, "issue:equipo/despliegues#1234");
        assert_eq!(wi.visible, "WI 1234");
        assert_eq!(
            wi.url,
            "https://gitlab.com/equipo/despliegues/-/issues/1234"
        );

        let wi_work_items = reconocido("https://gitlab.com/equipo/despliegues/-/work_items/1234");
        assert_eq!(
            wi_work_items.url,
            "https://gitlab.com/equipo/despliegues/-/issues/1234"
        );

        let fs = reconocido("https://empresa.freshservice.com/a/tickets/45600");
        assert_eq!(fs.tipo, TipoRecurso::Fs);
        assert_eq!(fs.clave, "fs:45600");
        assert_eq!(fs.visible, "FS#45600");
        assert_eq!(fs.url, "https://empresa.freshservice.com/a/tickets/45600");
        assert_eq!(fs.proyecto, None);

        let helpdesk = reconocido("https://empresa.freshservice.com/helpdesk/tickets/45600");
        assert_eq!(helpdesk.clave, "fs:45600");
        assert_eq!(
            helpdesk.url,
            "https://empresa.freshservice.com/a/tickets/45600"
        );
    }

    #[test]
    fn variantes_de_gitlab_que_siguen_siendo_el_mismo_recurso() {
        for url in [
            "https://gitlab.com/a/b/-/merge_requests/39/diffs",
            "https://gitlab.com/a/b/-/merge_requests/39/diffs?tab=changes",
            "https://gitlab.com/a/b/-/merge_requests/39#note_123",
            "https://gitlab.com/a/b/-/merge_requests/39/?utm_source=correo",
            "HTTPS://GITLAB.COM/a/b/-/merge_requests/39/",
        ] {
            let r = reconocer(url, &ajustes()).unwrap_or_else(|| panic!("debería reconocer {url}"));
            assert_eq!(r.clave, "mr:a/b!39", "para {url}");
        }
        for url in [
            "https://gitlab.com/a/b/-/issues/7/related",
            "https://gitlab.com/a/b/-/issues/7#note_1",
            "https://gitlab.com/a/b/-/work_items/7/",
        ] {
            let r = reconocer(url, &ajustes()).unwrap_or_else(|| panic!("debería reconocer {url}"));
            assert_eq!(r.clave, "issue:a/b#7", "para {url}");
        }
    }

    #[test]
    fn paginas_de_gitlab_sin_recurso_aportan_proyecto_y_app() {
        let url = "https://gitlab.com/equipo/informes-web/-/tree/main";
        assert!(reconocer(url, &ajustes()).is_none());
        assert_eq!(
            proyecto_gitlab(url, &ajustes()).as_deref(),
            Some("equipo/informes-web")
        );
        assert_eq!(
            app_de_ruta("equipo/informes-web"),
            Some("informes-web".into())
        );
        assert_eq!(app_de_ruta("grupo/despliegues"), None);
        assert!(es_repo_wi("grupo/despliegues"));
        assert!(es_repo_wi("equipo/despliegues.git"));
        assert!(!es_repo_wi("grupo/otra-app"));
        assert_eq!(
            app_de_ruta("grupo/Descarga-Notas.git"),
            Some("descarga-notas".into())
        );
        assert_eq!(app_de_ruta(""), None);
    }

    #[test]
    fn normaliza_urls() {
        assert_eq!(
            normalizar("HTTPS://GITLAB.COM/a/b/?utm_source=x&tab=1#note_9"),
            "https://gitlab.com/a/b?tab=1"
        );
        assert_eq!(
            normalizar("https://x.es/a?fbclid=1&gclid=2&focusedCommentId=3&focusedId=4"),
            "https://x.es/a"
        );
        assert_eq!(normalizar("https://x.es/"), "https://x.es/");
        assert_eq!(normalizar("https://x.es"), "https://x.es/");
        assert_eq!(normalizar("  https://x.es/a/  "), "https://x.es/a");
        assert_eq!(normalizar("no es una url"), "no es una url");
        assert_eq!(normalizar("mailto:alguien@x.es"), "mailto:alguien@x.es");
    }

    #[test]
    fn urls_que_no_son_recursos() {
        for url in [
            "https://gitlab.com/grupo",
            "https://gitlab.com/",
            "https://ejemplo.com/browse/PROJ-1",
            "https://empresa.freshservice.com/a/tickets/abc",
            "https://empresa.atlassian.net/secure/Dashboard.jspa",
            "no es una url",
            "mailto:alguien@x.es",
            "",
        ] {
            assert!(reconocer(url, &ajustes()).is_none(), "para {url}");
        }
        // Un último segmento con forma de clave sí es recurso Jira.
        assert_eq!(
            reconocido("https://empresa.atlassian.net/jira/core/projects/PROJ/PROJ-2272").clave,
            "jira:PROJ-2272"
        );
    }

    #[test]
    fn fuentes_por_host() {
        let aj = ajustes();
        assert_eq!(
            fuente_de_url("https://empresa.atlassian.net/browse/PROJ-1", &aj),
            Fuente::Jira
        );
        assert_eq!(
            fuente_de_url("https://empresa.atlassian.net/wiki/x", &aj),
            Fuente::Jira
        );
        assert_eq!(
            fuente_de_url("https://gitlab.com/grupo/proy", &aj),
            Fuente::Gitlab
        );
        assert_eq!(
            fuente_de_url("https://empresa.freshservice.com/a/tickets/1", &aj),
            Fuente::Fs
        );
        assert_eq!(fuente_de_url("https://ejemplo.com/x", &aj), Fuente::Web);
        assert_eq!(fuente_de_url("no es una url", &aj), Fuente::Web);
    }

    #[test]
    fn claves_visibles() {
        assert_eq!(visible("jira:PROJ-1"), "PROJ-1");
        assert_eq!(visible("mr:a/b!3"), "!3");
        assert_eq!(visible("issue:equipo/despliegues#9"), "WI 9");
        assert_eq!(visible("issue:a/b#9"), "#9");
        assert_eq!(visible("fs:5"), "FS#5");
        assert_eq!(visible("wi:9"), "WI 9");
    }

    #[test]
    fn marcar_visto_copia_la_huella_y_limpia_los_cambios() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url, huella_actual, huella_vista, cambios)
             VALUES ('jira:PROJ-1', 'jira', 'https://x/browse/PROJ-1', 'actual', 'vieja', '[]')",
            [],
        )
        .expect("inserta recurso");
        let ahora = Local.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap();
        marcar_visto(&conn, "jira:PROJ-1", ahora).expect("marca visto");
        let (actual, vista, cambios, visto, pendiente): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
        ) = conn
            .query_row(
                "SELECT huella_actual, huella_vista, cambios, visto, pendiente_visto
                   FROM recurso WHERE clave = 'jira:PROJ-1'",
                [],
                |fila| {
                    Ok((
                        fila.get(0)?,
                        fila.get(1)?,
                        fila.get(2)?,
                        fila.get(3)?,
                        fila.get(4)?,
                    ))
                },
            )
            .expect("lee recurso");
        assert_eq!(actual.as_deref(), Some("actual"));
        assert_eq!(vista.as_deref(), Some("actual"));
        assert_eq!(cambios, None);
        assert_eq!(visto.as_deref(), Some(ahora.to_rfc3339().as_str()));
        assert_eq!(pendiente, 0);
    }

    #[test]
    fn marcar_visto_si_nuevo_no_borra_cambios_pendientes() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url, huella_actual, huella_vista, cambios, visto)
             VALUES ('jira:PROJ-1', 'jira', 'https://x/browse/PROJ-1', 'actual', 'vieja',
                     '[{\"tipo\":\"titulo\"}]', '2026-11-01T10:00:00+01:00'),
                    ('fs:2', 'fs', 'https://x/a/tickets/2', NULL, NULL, NULL, NULL)",
            [],
        )
        .expect("inserta recursos");
        let ahora = Local.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap();
        marcar_visto_si_nuevo(&conn, "jira:PROJ-1", ahora).expect("visto si nuevo");
        marcar_visto_si_nuevo(&conn, "fs:2", ahora).expect("visto si nuevo");
        let (vista, cambios): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT huella_vista, cambios FROM recurso WHERE clave = 'jira:PROJ-1'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("lee recurso");
        assert_eq!(vista.as_deref(), Some("vieja"));
        assert!(cambios.is_some());
        let (visto, pendiente): (Option<String>, i64) = conn
            .query_row(
                "SELECT visto, pendiente_visto FROM recurso WHERE clave = 'fs:2'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("lee recurso nuevo");
        assert!(visto.is_some());
        assert_eq!(pendiente, 1);
    }

    #[test]
    fn marcar_visto_sin_huella_deja_pendiente() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url) VALUES ('fs:1', 'fs', 'https://x/a/tickets/1')",
            [],
        )
        .expect("inserta recurso");
        let ahora = Local.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap();
        marcar_visto(&conn, "fs:1", ahora).expect("marca visto");
        let (vista, pendiente): (Option<String>, i64) = conn
            .query_row(
                "SELECT huella_vista, pendiente_visto FROM recurso WHERE clave = 'fs:1'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("lee recurso");
        assert_eq!(vista, None);
        assert_eq!(pendiente, 1);
        // Marcar un recurso que no existe tampoco falla.
        marcar_visto(&conn, "fs:999", ahora).expect("no falla");
    }
}
