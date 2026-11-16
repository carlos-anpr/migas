//! Ajustes (`~/.config/migas/settings.json`) y credenciales (entorno o
//! `~/.config/migas/credenciales.env`, solo lectura local).
//!
//! Mismo patrón que `hilo`: fusión sobre el valor por defecto y escritura atómica.
//! Las credenciales viven solo en memoria y su `Debug` sale redactado.

use crate::error::{Error, Resultado};
use crate::model::Ajustes;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Credenciales en memoria. NUNCA se serializan, registran ni mandan al frontend.
/// Cada campo es opcional: si falta, solo falla la fuente que lo necesita.
#[derive(Clone, Default)]
pub struct Credenciales {
    pub jira_user: Option<String>,
    pub jira_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub fs_key: Option<String>,
    pub ia_key: Option<String>,
}

impl std::fmt::Debug for Credenciales {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credenciales(**redactadas**)")
    }
}

impl Credenciales {
    /// Lee las credenciales del home del usuario. Nunca falla: lo que falte queda en `None`.
    pub fn cargar() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        cargar_desde(&home, |clave| std::env::var(clave).ok())
    }
}

/// Variables de credenciales. Valen en el entorno o en `~/.config/migas/credenciales.env`;
/// el entorno manda sobre el fichero.
pub const VAR_JIRA_USER: &str = "JIRA_USER";
pub const VAR_JIRA_TOKEN: &str = "JIRA_API_TOKEN";
pub const VAR_GITLAB_TOKEN: &str = "GITLAB_TOKEN";
pub const VAR_FS_KEY: &str = "FRESHSERVICE_API_KEY";
pub const VAR_IA_KEY: &str = "MIGAS_IA_API_KEY";
/// Modelo y endpoint de la IA: mandan sobre los de los ajustes.
pub const VAR_IA_MODELO: &str = "MIGAS_IA_MODEL";
pub const VAR_IA_BASE_URL: &str = "MIGAS_IA_BASE_URL";

/// Modelo y URL base efectivos de la IA: la variable de entorno si existe y no está vacía;
/// si no, los de los ajustes.
pub fn ia_efectiva(aj: &Ajustes, env: impl Fn(&str) -> Option<String>) -> (String, String) {
    let elegir = |clave: &str, ajuste: &str| {
        env(clave)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| ajuste.trim().to_string())
    };
    (
        elegir(VAR_IA_MODELO, &aj.ia_modelo),
        elegir(VAR_IA_BASE_URL, &aj.ia_base_url),
    )
}

/// Lee las credenciales de un `home` concreto y del entorno `env` (las pruebas inyectan
/// ambos). Lo que no esté en ningún sitio queda en `None`.
pub fn cargar_desde(home: &Path, env: impl Fn(&str) -> Option<String>) -> Credenciales {
    let fichero = leer_env(&home.join(".config").join("migas").join("credenciales.env"));
    let valor = |clave: &str| {
        env(clave)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .or_else(|| valor_env(&fichero, clave))
    };
    Credenciales {
        jira_user: valor(VAR_JIRA_USER),
        jira_token: valor(VAR_JIRA_TOKEN),
        gitlab_token: valor(VAR_GITLAB_TOKEN),
        fs_key: valor(VAR_FS_KEY),
        ia_key: valor(VAR_IA_KEY),
    }
}

/// Ruta del fichero de ajustes: `~/.config/migas/settings.json`.
pub fn ruta_ajustes() -> PathBuf {
    let base =
        dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("migas").join("settings.json")
}

/// Carga los ajustes del usuario. Si no hay fichero o no se puede leer → valores por defecto.
pub fn cargar_ajustes() -> Ajustes {
    cargar_ajustes_de(&ruta_ajustes())
}

/// Carga los ajustes de un fichero concreto (las pruebas usan un directorio temporal).
pub fn cargar_ajustes_de(ruta: &Path) -> Ajustes {
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return Ajustes::default();
    };
    let Ok(valor) = serde_json::from_str::<Value>(&texto) else {
        return Ajustes::default();
    };
    fusionar_con_default(valor)
}

/// Guarda los ajustes del usuario de forma atómica (fichero temporal + rename).
pub fn guardar_ajustes(ajustes: &Ajustes) -> Resultado<()> {
    guardar_ajustes_en(&ruta_ajustes(), ajustes)
}

/// Guarda los ajustes en la ruta dada, creando la carpeta si hace falta.
pub fn guardar_ajustes_en(ruta: &Path, ajustes: &Ajustes) -> Resultado<()> {
    let texto = serde_json::to_string_pretty(ajustes)?;
    let carpeta = ruta
        .parent()
        .ok_or_else(|| Error::Validacion("Ruta de ajustes inválida".into()))?;
    std::fs::create_dir_all(carpeta)?;
    let temporal = ruta.with_extension("json.tmp");
    std::fs::write(&temporal, texto.as_bytes())?;
    std::fs::rename(&temporal, ruta)?;
    Ok(())
}

/// Carpeta de datos: `MIGAS_DATA_DIR` si existe; si no, `dirs::data_dir()/migas`.
pub fn dir_datos() -> PathBuf {
    dir_datos_desde(std::env::var_os("MIGAS_DATA_DIR").as_deref())
}

fn dir_datos_desde(valor: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(valor) = valor {
        let ruta = PathBuf::from(valor);
        if !ruta.as_os_str().is_empty() {
            return ruta;
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".local")
                .join("share")
        })
        .join("migas")
}

/// Expande `~` y `~/…` al home del usuario. El resto se devuelve tal cual.
pub fn expandir_tilde(ruta: &str) -> PathBuf {
    let ruta = ruta.trim();
    if ruta == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(resto) = ruta.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(resto);
    }
    PathBuf::from(ruta)
}

/// Fusiona lo leído con los valores por defecto: los campos ausentes conservan el default.
/// Si algún campo viene con un tipo raro, se usan los valores por defecto enteros.
fn fusionar_con_default(valor: Value) -> Ajustes {
    let Ok(Value::Object(base)) = serde_json::to_value(Ajustes::default()) else {
        return Ajustes::default();
    };
    let Value::Object(extra) = valor else {
        return Ajustes::default();
    };
    let mut mapa = base;
    for (clave, valor) in extra {
        mapa.insert(clave, valor);
    }
    serde_json::from_value(Value::Object(mapa)).unwrap_or_default()
}

/// Lee un fichero `CLAVE=valor` (también `export CLAVE=valor`). Ignora líneas vacías,
/// comentarios `#` y líneas sin `=`; quita las comillas simples o dobles envolventes.
fn leer_env(ruta: &Path) -> HashMap<String, String> {
    let mut mapa = HashMap::new();
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return mapa;
    };
    for linea in texto.lines() {
        let linea = linea.trim();
        if linea.is_empty() || linea.starts_with('#') {
            continue;
        }
        let linea = linea.strip_prefix("export ").unwrap_or(linea).trim_start();
        let Some((clave, valor)) = linea.split_once('=') else {
            continue;
        };
        let clave = clave.trim();
        if clave.is_empty() {
            continue;
        }
        mapa.insert(clave.to_string(), quitar_comillas(valor.trim()).to_string());
    }
    mapa
}

/// Quita las comillas simples o dobles que envuelven el valor.
fn quitar_comillas(valor: &str) -> &str {
    let bytes = valor.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        &valor[1..valor.len() - 1]
    } else {
        valor
    }
}

/// Valor de una clave del fichero `.env`, si existe y no está vacío.
fn valor_env(mapa: &HashMap<String, String>, clave: &str) -> Option<String> {
    mapa.get(clave)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escribir(ruta: &Path, contenido: &str) {
        if let Some(carpeta) = ruta.parent() {
            std::fs::create_dir_all(carpeta).expect("carpeta de prueba");
        }
        std::fs::write(ruta, contenido).expect("fichero de prueba");
    }

    fn sin_entorno(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn lee_el_fichero_de_credenciales() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let home = tmp.path();
        escribir(
            &home.join(".config").join("migas").join("credenciales.env"),
            "# credenciales de prueba\n\
             JIRA_USER=usuario\n\
             \n\
             export JIRA_API_TOKEN=\"token-de-prueba\"\n\
             GITLAB_TOKEN='otro-token'\n\
             FRESHSERVICE_API_KEY=fs-de-prueba\n\
             MIGAS_IA_API_KEY=ia-de-prueba\n\
             linea_sin_igual\n\
             =sin_clave\n",
        );

        let cred = cargar_desde(home, sin_entorno);
        assert_eq!(cred.jira_user.as_deref(), Some("usuario"));
        assert_eq!(cred.jira_token.as_deref(), Some("token-de-prueba"));
        assert_eq!(cred.gitlab_token.as_deref(), Some("otro-token"));
        assert_eq!(cred.fs_key.as_deref(), Some("fs-de-prueba"));
        assert_eq!(cred.ia_key.as_deref(), Some("ia-de-prueba"));
        assert_eq!(format!("{cred:?}"), "Credenciales(**redactadas**)");
        let debug = format!("{cred:?}");
        assert!(!debug.contains("token-de-prueba"));
        assert!(!debug.contains("usuario"));
    }

    #[test]
    fn el_entorno_manda_sobre_el_fichero() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let home = tmp.path();
        escribir(
            &home.join(".config").join("migas").join("credenciales.env"),
            "MIGAS_IA_API_KEY=del-fichero\nGITLAB_TOKEN=gl-del-fichero\n",
        );
        let cred = cargar_desde(home, |clave| match clave {
            "MIGAS_IA_API_KEY" => Some("del-entorno".into()),
            "GITLAB_TOKEN" => Some("  ".into()),
            _ => None,
        });
        assert_eq!(cred.ia_key.as_deref(), Some("del-entorno"));
        assert_eq!(cred.gitlab_token.as_deref(), Some("gl-del-fichero"));
    }

    #[test]
    fn modelo_y_url_de_la_ia_por_entorno() {
        let aj = Ajustes::default();
        assert_eq!(
            ia_efectiva(&aj, sin_entorno),
            (aj.ia_modelo.clone(), aj.ia_base_url.clone())
        );
        let (modelo, url) = ia_efectiva(&aj, |clave| match clave {
            "MIGAS_IA_MODEL" => Some(" gpt-5-mini ".into()),
            "MIGAS_IA_BASE_URL" => Some("".into()),
            _ => None,
        });
        assert_eq!(modelo, "gpt-5-mini");
        assert_eq!(url, aj.ia_base_url);
    }

    #[test]
    fn sin_ficheros_no_hay_credenciales_pero_no_hay_error() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let cred = cargar_desde(tmp.path(), sin_entorno);
        assert!(cred.jira_user.is_none());
        assert!(cred.jira_token.is_none());
        assert!(cred.gitlab_token.is_none());
        assert!(cred.fs_key.is_none());
        assert!(cred.ia_key.is_none());
    }

    #[test]
    fn valores_vacios_o_ausentes_quedan_en_none() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let home = tmp.path();
        escribir(
            &home.join(".config").join("migas").join("credenciales.env"),
            "JIRA_USER=\nJIRA_API_TOKEN=\"\"\n",
        );
        let cred = cargar_desde(home, sin_entorno);
        assert!(cred.jira_user.is_none());
        assert!(cred.jira_token.is_none());
        assert!(cred.gitlab_token.is_none());
        assert!(cred.fs_key.is_none());
        assert!(cred.ia_key.is_none());
    }

    #[test]
    fn ajustes_ausentes_usan_los_valores_por_defecto() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ajustes = cargar_ajustes_de(&tmp.path().join("settings.json"));
        assert_eq!(ajustes, Ajustes::default());
    }

    #[test]
    fn json_parcial_conserva_los_campos_ausentes() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("settings.json");
        escribir(
            &ruta,
            r#"{"jiraBaseUrl":"https://jira.de-prueba","tema":"oscuro","diasSeguimiento":7}"#,
        );
        let ajustes = cargar_ajustes_de(&ruta);
        assert_eq!(ajustes.jira_base_url, "https://jira.de-prueba");
        assert_eq!(ajustes.tema, "oscuro");
        assert_eq!(ajustes.dias_seguimiento, 7);
        assert_eq!(ajustes.gitlab_base_url, Ajustes::default().gitlab_base_url);
        assert_eq!(ajustes.intervalo_min, Ajustes::default().intervalo_min);
    }

    #[test]
    fn json_roto_o_de_tipo_raro_usa_los_valores_por_defecto() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let rota = tmp.path().join("rota.json");
        escribir(&rota, "{esto no es json");
        assert_eq!(cargar_ajustes_de(&rota), Ajustes::default());

        let raro = tmp.path().join("raro.json");
        escribir(&raro, r#"{"intervaloMin":"muchos"}"#);
        assert_eq!(
            cargar_ajustes_de(&raro).intervalo_min,
            Ajustes::default().intervalo_min
        );
    }

    #[test]
    fn guarda_y_vuelve_a_cargar() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas").join("settings.json");
        let ajustes = Ajustes {
            tema: "oscuro".into(),
            apps: vec!["informes-web".into()],
            ..Ajustes::default()
        };

        guardar_ajustes_en(&ruta, &ajustes).expect("guarda los ajustes");
        assert!(ruta.exists(), "el fichero de ajustes debería existir");
        assert!(
            !ruta.with_extension("json.tmp").exists(),
            "el temporal no debería quedarse"
        );

        let leidos = cargar_ajustes_de(&ruta);
        assert_eq!(leidos, ajustes);
    }

    #[test]
    fn dir_datos_respeta_la_variable_y_el_valor_por_defecto() {
        let temporal = std::ffi::OsStr::new("/tmp/migas-de-prueba");
        assert_eq!(dir_datos_desde(Some(temporal)), PathBuf::from(temporal));
        // Una variable vacía no cuenta como configurada.
        assert!(dir_datos_desde(Some(std::ffi::OsStr::new(""))).ends_with("migas"));
        assert!(dir_datos_desde(None).ends_with("migas"));
    }

    #[test]
    fn expande_la_tilde() {
        let home = dirs::home_dir().expect("home");
        assert_eq!(expandir_tilde("~/Proyectos"), home.join("Proyectos"));
        assert_eq!(expandir_tilde("~"), home);
        assert_eq!(expandir_tilde("/tmp/otro"), PathBuf::from("/tmp/otro"));
        assert_eq!(expandir_tilde("relativa/x"), PathBuf::from("relativa/x"));
    }
}
