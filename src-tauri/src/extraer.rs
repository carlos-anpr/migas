//! [T1] Extracción de referencias (Jira, FS, WI, MR) de texto libre.

use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::sync::LazyLock;

/// Referencia encontrada en un texto. El orden de las variantes fija el orden de salida de
/// `extraer`: primero Jira, después FS, WI y MR.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ref {
    Jira(String),
    Fs(u64),
    Wi(u64),
    Mr { proyecto: String, iid: u64 },
}

/// `PROJ-2272`, `proj-2272`. Solo cuenta si el prefijo está entre los proyectos Jira leídos.
static RE_JIRA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([a-z][a-z0-9]{1,9})-(\d{1,6})\b").expect("regex de Jira"));

/// `https://empresa.freshservice.com/a/tickets/45679`.
static RE_FS_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)freshservice\.com/a/tickets/(\d+)").expect("regex de URL de FS")
});

/// `FS #45679`, `FS#45679`.
static RE_FS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bFS\s*#?\s*(\d{4,6})\b").expect("regex de FS"));

/// `#INC-45680`, `INC - 45681`, `INC-45679-Notas`.
static RE_INC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bINC\s*-?\s*(\d{5,6})\b").expect("regex de INC"));

/// `<repoWi>/-/work_items/14` y la variante `issues`: el grupo 1 es el repositorio.
static RE_WI_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)/([\w.-]+)/-/(?:work_items|issues)/(\d+)").expect("regex de URL de WI")
});

/// `WI 1235`, `WI#1235`.
static RE_WI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bWI\s*#?\s*(\d{4,6})\b").expect("regex de WI"));

/// `https://host/grupo/proyecto/-/merge_requests/203`.
static RE_MR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)https?://[^/\s]+/([\w./-]+?)/-/merge_requests/(\d+)").expect("regex de MR")
});

/// Número por debajo del cual un `FS#n` es en realidad una WI (error de prefijo conocido).
const FS_QUE_SON_WI: u64 = 30_000;

/// Referencias de un texto, sin duplicados y en orden estable (el de [`Ref`]).
///
/// `proyectos_jira` son los prefijos de las issues leídas, en mayúsculas (`PROJ`, `OTRO`…).
/// Las WI solo se reconocen si hay repositorio de work items (ajuste `repoWi`). Con él, un
/// `FS#n` con `n < 30000` se devuelve como [`Ref::Wi`]: es el error de prefijo habitual
/// (`FS#1237` es la WI 1237).
pub fn extraer(texto: &str, proyectos_jira: &HashSet<String>) -> Vec<Ref> {
    let mut salida = BTreeSet::new();
    let repo_wi = crate::recurso::repo_wi();
    let con_wi = !repo_wi.is_empty();

    for c in RE_JIRA.captures_iter(texto) {
        let (Some(prefijo), Some(numero)) = (c.get(1), c.get(2)) else {
            continue;
        };
        let prefijo = prefijo.as_str().to_uppercase();
        if !proyectos_jira.contains(&prefijo) {
            continue;
        }
        let Ok(numero) = numero.as_str().parse::<u64>() else {
            continue;
        };
        salida.insert(Ref::Jira(format!("{prefijo}-{numero}")));
    }

    for c in RE_FS_URL.captures_iter(texto) {
        if let Some(id) = numero(&c, 1) {
            salida.insert(Ref::Fs(id));
        }
    }

    for c in RE_FS.captures_iter(texto) {
        if let Some(id) = numero(&c, 1) {
            if con_wi && id < FS_QUE_SON_WI {
                salida.insert(Ref::Wi(id));
            } else {
                salida.insert(Ref::Fs(id));
            }
        }
    }

    for c in RE_INC.captures_iter(texto) {
        if let Some(id) = numero(&c, 1) {
            salida.insert(Ref::Fs(id));
        }
    }

    for c in RE_WI_URL.captures_iter(texto) {
        let repo = c.get(1).map(|m| m.as_str()).unwrap_or("");
        if !con_wi || !repo.eq_ignore_ascii_case(&repo_wi) {
            continue;
        }
        if let Some(iid) = numero(&c, 2) {
            salida.insert(Ref::Wi(iid));
        }
    }

    for c in RE_WI.captures_iter(texto) {
        if !con_wi {
            break;
        }
        if let Some(iid) = numero(&c, 1) {
            salida.insert(Ref::Wi(iid));
        }
    }

    for c in RE_MR.captures_iter(texto) {
        let (Some(proyecto), Some(iid)) = (c.get(1), c.get(2)) else {
            continue;
        };
        let Ok(iid) = iid.as_str().parse::<u64>() else {
            continue;
        };
        let proyecto = proyecto.as_str().trim_matches('/').to_string();
        if !proyecto.is_empty() {
            salida.insert(Ref::Mr { proyecto, iid });
        }
    }

    salida.into_iter().collect()
}

fn numero(c: &regex::Captures<'_>, grupo: usize) -> Option<u64> {
    c.get(grupo).and_then(|m| m.as_str().parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proyectos() -> HashSet<String> {
        ["PROJ".to_string(), "GLIT".to_string()]
            .into_iter()
            .collect()
    }

    #[test]
    fn jira() {
        let p = proyectos();
        assert_eq!(
            extraer("PROJ-2272: listado", &p),
            vec![Ref::Jira("PROJ-2272".into())]
        );
        // En ramas va en minúsculas y se normaliza.
        assert_eq!(
            extraer("rama proj-2272 pusheada", &p),
            vec![Ref::Jira("PROJ-2272".into())]
        );
        // Un prefijo desconocido no es una referencia.
        assert!(extraer("UTF-8 y AES-256", &p).is_empty());
        // Ni una rama con número pero sin prefijo de proyecto.
        assert!(extraer("feat/12397-pre-run", &p).is_empty());
    }

    #[test]
    fn fs() {
        let p = proyectos();
        assert_eq!(
            extraer("#INC-45680 Nuevo listado", &p),
            vec![Ref::Fs(45680)]
        );
        assert_eq!(
            extraer("INC - 45681 - Incompatibilidades", &p),
            vec![Ref::Fs(45681)]
        );
        assert_eq!(extraer("INC-45679-Notas", &p), vec![Ref::Fs(45679)]);
        assert_eq!(
            extraer(
                "[FS #45679](https://empresa.freshservice.com/a/tickets/45679)",
                &p
            ),
            vec![Ref::Fs(45679)]
        );
        assert_eq!(extraer("FS#45678", &p), vec![Ref::Fs(45678)]);
    }

    #[test]
    fn fs_pequeno_en_realidad_es_wi() {
        let p = proyectos();
        assert_eq!(extraer("FS#1237", &p), vec![Ref::Wi(1237)]);
        assert_eq!(extraer("FS #1234", &p), vec![Ref::Wi(1234)]);
        // El límite: 30000 ya es FS.
        assert_eq!(extraer("FS#30000", &p), vec![Ref::Fs(30000)]);
    }

    #[test]
    fn wi() {
        let p = proyectos();
        assert_eq!(
            extraer("https://gitlab.com/equipo/despliegues/-/work_items/14", &p),
            vec![Ref::Wi(14)]
        );
        assert_eq!(
            extraer("https://gitlab.com/equipo/despliegues/-/issues/1238", &p),
            vec![Ref::Wi(1238)]
        );
        assert_eq!(extraer("WI 1235 desplegada", &p), vec![Ref::Wi(1235)]);
        assert_eq!(extraer("WI#1236", &p), vec![Ref::Wi(1236)]);
    }

    #[test]
    fn mr() {
        let p = proyectos();
        assert_eq!(
            extraer(
                "[!203](https://gitlab.com/equipo/informes-web/-/merge_requests/203)",
                &p
            ),
            vec![Ref::Mr {
                proyecto: "equipo/informes-web".into(),
                iid: 203
            }]
        );
        assert_eq!(
            extraer("https://gitlab.com/a/b/c/-/merge_requests/35", &p),
            vec![Ref::Mr {
                proyecto: "a/b/c".into(),
                iid: 35
            }]
        );
    }

    #[test]
    fn sin_duplicados_y_orden_estable() {
        let p = proyectos();
        let texto = "PROJ-2413 ... FS #45679 ... https://empresa.freshservice.com/a/tickets/45679 ... \
                     WI 1236 ... https://x/equipo/despliegues/-/work_items/1236 ... PROJ-2413 otra vez";
        assert_eq!(
            extraer(texto, &p),
            vec![Ref::Jira("PROJ-2413".into()), Ref::Fs(45679), Ref::Wi(1236)]
        );
    }

    #[test]
    fn texto_sin_referencias() {
        let p = proyectos();
        assert!(extraer("Se ha pedido el despliegue y no hay más datos.", &p).is_empty());
        assert!(extraer("", &p).is_empty());
    }
}
