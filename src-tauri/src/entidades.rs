//! Entidades de una entrada (refs, app, proyecto) y vocabulario de apps conocidas.

use crate::db;
use crate::error::{Error, Resultado};
use crate::extraer::{Ref, extraer};
use crate::model::Ajustes;
use crate::recurso;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use url::Url;

/// De dónde sale una entidad. Coincide con el `CHECK` de `entidad.fuente`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuenteEntidad {
    Url,
    Texto,
    Nota,
}

impl FuenteEntidad {
    pub fn como_str(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Texto => "texto",
            Self::Nota => "nota",
        }
    }
}

/// Datos de una entrada que producen entidades.
#[derive(Debug, Clone, Default)]
pub struct EntradaParaEntidades {
    pub url: Option<String>,
    pub titulo: String,
    pub seleccion: Option<String>,
    pub notas: Vec<String>,
}

/// Prefijos Jira conocidos, vocabulario de apps y ajustes para reconocer URLs.
#[derive(Debug, Clone)]
pub struct Contexto {
    pub prefijos_jira: HashSet<String>,
    pub vocabulario: Vec<String>,
    pub ajustes: Ajustes,
}

impl Contexto {
    pub fn cargar(conn: &Connection, ajustes: &Ajustes) -> Resultado<Self> {
        let mut prefijos_jira: HashSet<String> = ajustes
            .prefijos_jira
            .iter()
            .map(|prefijo| prefijo.trim().to_uppercase())
            .filter(|prefijo| !prefijo.is_empty())
            .collect();
        let mut consulta = conn.prepare(
            "SELECT DISTINCT proyecto FROM recurso WHERE tipo = 'jira' AND proyecto IS NOT NULL",
        )?;
        let filas = consulta.query_map([], |fila| fila.get::<_, String>(0))?;
        for proyecto in filas {
            let proyecto = proyecto?;
            if !proyecto.trim().is_empty() {
                prefijos_jira.insert(proyecto.trim().to_uppercase());
            }
        }
        Ok(Self {
            prefijos_jira,
            vocabulario: leer_vocabulario(conn)?,
            ajustes: ajustes.clone(),
        })
    }
}

/// Texto normalizado para comparar: minúsculas, sin acentos y con `[-_./]` y los espacios
/// repetidos convertidos en un único espacio.
pub fn normalizar_texto(texto: &str) -> String {
    let mut salida = String::with_capacity(texto.len());
    for caracter in texto.chars() {
        for normal in normalizar_caracter(caracter) {
            if normal == ' ' && salida.ends_with(' ') {
                continue;
            }
            salida.push(normal);
        }
    }
    salida.trim().to_string()
}

fn normalizar_caracter(caracter: char) -> Vec<char> {
    if caracter.is_whitespace() || matches!(caracter, '-' | '_' | '.' | '/') {
        return vec![' '];
    }
    let mut salida = Vec::new();
    for minuscula in caracter.to_lowercase() {
        match minuscula {
            '\u{0300}'..='\u{036f}' => {}
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => salida.push('a'),
            'é' | 'è' | 'ë' | 'ê' => salida.push('e'),
            'í' | 'ì' | 'ï' | 'î' => salida.push('i'),
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => salida.push('o'),
            'ú' | 'ù' | 'ü' | 'û' => salida.push('u'),
            'ñ' => salida.push('n'),
            'ç' => salida.push('c'),
            otro => salida.push(otro),
        }
    }
    salida
}

/// Apps del vocabulario que aparecen en el texto con límites de palabra. Se ignoran los
/// nombres de menos de 5 caracteres.
pub fn apps_en_texto(texto: &str, vocabulario: &[String]) -> Vec<String> {
    let mut salida: Vec<String> = Vec::new();
    for app in vocabulario {
        let aguja = normalizar_texto(app);
        if aguja.chars().count() < 5 {
            continue;
        }
        if !contiene_con_limites(texto, &aguja) {
            continue;
        }
        let app = app.trim().to_lowercase();
        if !app.is_empty() && !salida.contains(&app) {
            salida.push(app);
        }
    }
    salida
}

/// `true` si `aguja` (ya normalizada) aparece en `texto` sin pegarse a letras, números ni
/// separadores de palabra (`-`, `_`, `.`, `/`). Así `notas` no casa con `notas-simples-x`.
fn contiene_con_limites(texto: &str, aguja: &str) -> bool {
    let aguja: Vec<char> = aguja.chars().collect();
    if aguja.is_empty() {
        return false;
    }
    let (haystack, mapa) = normalizar_con_mapa(texto);
    if aguja.len() > haystack.len() {
        return false;
    }
    for inicio in 0..=haystack.len() - aguja.len() {
        if haystack[inicio..inicio + aguja.len()] != aguja[..] {
            continue;
        }
        let primer_byte = mapa[inicio];
        let ultimo_byte = mapa[inicio + aguja.len() - 1];
        let fin_byte = ultimo_byte + caracter_en(texto, ultimo_byte).map_or(0, char::len_utf8);
        let antes_ok = texto[..primer_byte]
            .chars()
            .next_back()
            .is_none_or(es_limite);
        let despues_ok = texto[fin_byte..].chars().next().is_none_or(es_limite);
        if antes_ok && despues_ok {
            return true;
        }
    }
    false
}

fn caracter_en(texto: &str, byte: usize) -> Option<char> {
    texto[byte..].chars().next()
}

fn es_limite(caracter: char) -> bool {
    !(caracter.is_alphanumeric() || matches!(caracter, '-' | '_' | '.' | '/'))
}

/// Texto normalizado carácter a carácter, con el byte original que produjo cada uno.
fn normalizar_con_mapa(texto: &str) -> (Vec<char>, Vec<usize>) {
    let mut salida: Vec<char> = Vec::new();
    let mut mapa: Vec<usize> = Vec::new();
    for (byte, caracter) in texto.char_indices() {
        for normal in normalizar_caracter(caracter) {
            if normal == ' ' && salida.last() == Some(&' ') {
                continue;
            }
            salida.push(normal);
            mapa.push(byte);
        }
    }
    let inicio = salida
        .iter()
        .position(|caracter| *caracter != ' ')
        .unwrap_or(salida.len());
    salida.drain(..inicio);
    mapa.drain(..inicio);
    let fin = salida
        .iter()
        .rposition(|caracter| *caracter != ' ')
        .map_or(0, |indice| indice + 1);
    salida.truncate(fin);
    mapa.truncate(fin);
    (salida, mapa)
}

static RE_ENLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("regex de enlace markdown"));
static RE_NOMBRE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9._-]*$").expect("regex de nombre de proyecto"));

/// App de una WI según la sección `## Aplicación` de su descripción. Primera app válida.
pub fn app_de_wi(descripcion: &str) -> Option<String> {
    let mut dentro = false;
    let mut primera = true;
    for linea in descripcion.lines() {
        let limpia = linea.trim();
        if let Some(titulo) = encabezado(limpia) {
            dentro = normalizar_texto(titulo) == "aplicacion";
            primera = true;
            continue;
        }
        if !dentro || limpia.is_empty() {
            continue;
        }
        if limpia.ends_with('?') || es_casilla(limpia) {
            break;
        }
        let cuenta = primera || empieza_por_lista(limpia) || limpia.starts_with('[');
        primera = false;
        if !cuenta {
            continue;
        }
        if let Some(nombre) = nombre_de_linea(limpia) {
            return Some(nombre);
        }
    }
    None
}

fn encabezado(linea: &str) -> Option<&str> {
    let almohadillas = linea
        .chars()
        .take_while(|caracter| *caracter == '#')
        .count();
    if almohadillas == 0 || almohadillas > 6 {
        return None;
    }
    Some(linea[almohadillas..].trim())
}

fn es_casilla(linea: &str) -> bool {
    let resto = linea
        .strip_prefix(['-', '*', '+'])
        .map(str::trim_start)
        .unwrap_or(linea);
    ["[ ]", "[]", "[x]", "[X]"]
        .iter()
        .any(|marca| resto.starts_with(marca))
}

fn empieza_por_lista(linea: &str) -> bool {
    if linea.starts_with(['-', '*', '+']) {
        return true;
    }
    let digitos = linea.chars().take_while(char::is_ascii_digit).count();
    digitos > 0 && linea[digitos..].starts_with(['.', ')'])
}

fn nombre_de_linea(linea: &str) -> Option<String> {
    let sin_enlace = RE_ENLACE.replace_all(linea, "$1").to_string();
    let mut resto = sin_enlace.trim();
    if let Some(siguiente) = resto.strip_prefix(['-', '*', '+']) {
        resto = siguiente.trim_start();
    } else {
        let digitos = resto.chars().take_while(char::is_ascii_digit).count();
        if digitos > 0
            && let Some(siguiente) = resto[digitos..]
                .strip_prefix('.')
                .or_else(|| resto[digitos..].strip_prefix(')'))
        {
            resto = siguiente.trim_start();
        }
    }
    let palabra = resto.split_whitespace().next()?;
    let palabra = palabra.split('(').next().unwrap_or(palabra);
    let nombre = palabra
        .trim_matches(['*', '_', '`', '"', '\'', '[', ']', '(', ')', '<', '>'])
        .trim_end_matches(['.', ',', ';', ':'])
        .to_lowercase();
    RE_NOMBRE.is_match(&nombre).then_some(nombre)
}

/// Entidades de una entrada, con su fuente. Primero la URL, luego título+selección y notas.
pub fn de_entrada(entrada: &EntradaParaEntidades, ctx: &Contexto) -> Vec<(String, FuenteEntidad)> {
    let mut salida: Vec<(String, FuenteEntidad)> = Vec::new();

    if let Some(url) = entrada.url.as_deref() {
        if let Some(reconocido) = recurso::reconocer(url, &ctx.ajustes) {
            anadir(&mut salida, reconocido.clave.clone(), FuenteEntidad::Url);
            if reconocido.es_wi
                && let Some((_, numero)) = reconocido.clave.rsplit_once('#')
            {
                anadir(&mut salida, format!("wi:{numero}"), FuenteEntidad::Url);
            }
        }
        if let Some(proyecto) = recurso::proyecto_gitlab(url, &ctx.ajustes) {
            if let Some(app) = recurso::app_de_ruta(&proyecto) {
                // Preferimos la app de la URL al proyecto para que la relación sea más fina.
                anadir(&mut salida, format!("app:{app}"), FuenteEntidad::Url);
            }
            anadir(
                &mut salida,
                format!("proyecto:{proyecto}"),
                FuenteEntidad::Url,
            );
        }
    }

    let mut texto = entrada.titulo.clone();
    if let Some(seleccion) = entrada.seleccion.as_deref() {
        texto.push('\n');
        texto.push_str(seleccion);
    }
    anadir_texto(
        &mut salida,
        &texto,
        &ctx.prefijos_jira,
        &ctx.vocabulario,
        FuenteEntidad::Texto,
    );

    let mut notas = String::new();
    for nota in &entrada.notas {
        notas.push('\n');
        notas.push_str(nota);
    }
    anadir_texto(
        &mut salida,
        &notas,
        &ctx.prefijos_jira,
        &ctx.vocabulario,
        FuenteEntidad::Nota,
    );

    salida
}

fn anadir_texto(
    salida: &mut Vec<(String, FuenteEntidad)>,
    texto: &str,
    prefijos: &HashSet<String>,
    vocabulario: &[String],
    fuente: FuenteEntidad,
) {
    for referencia in extraer(texto, prefijos) {
        anadir(salida, valor_de_ref(&referencia), fuente);
    }
    for app in apps_en_texto(texto, vocabulario) {
        anadir(salida, format!("app:{app}"), fuente);
    }
}

fn anadir(salida: &mut Vec<(String, FuenteEntidad)>, valor: String, fuente: FuenteEntidad) {
    if recurso::es_entidad_prohibida(&valor) {
        return;
    }
    if !valor.is_empty() && !salida.iter().any(|(existente, _)| existente == &valor) {
        salida.push((valor, fuente));
    }
}

/// Traduce una referencia de texto a su valor de entidad.
pub fn valor_de_ref(referencia: &Ref) -> String {
    match referencia {
        Ref::Jira(clave) => format!("jira:{clave}"),
        Ref::Fs(id) => format!("fs:{id}"),
        Ref::Wi(id) => format!("wi:{id}"),
        Ref::Mr { proyecto, iid } => format!("mr:{proyecto}!{iid}"),
    }
}

/// Reescribe las entidades de una entrada: `url`, `texto` (título+selección) y `nota`.
pub fn recalcular(conn: &Connection, id: i64, ctx: &Contexto) -> Resultado<()> {
    let fila: Option<(Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT url, titulo, seleccion FROM entrada WHERE id = ?1",
            [id],
            |fila| Ok((fila.get(0)?, fila.get(1)?, fila.get(2)?)),
        )
        .optional()?;
    let Some((url, titulo, seleccion)) = fila else {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    };
    let mut consulta =
        conn.prepare("SELECT texto FROM nota WHERE entrada_id = ?1 ORDER BY creada, id")?;
    let notas = consulta
        .query_map([id], |fila| fila.get::<_, String>(0))?
        .collect::<Result<Vec<String>, _>>()?;

    let entidades = de_entrada(
        &EntradaParaEntidades {
            url,
            titulo,
            seleccion,
            notas,
        },
        ctx,
    );
    conn.execute("DELETE FROM entidad WHERE entrada_id = ?1", [id])?;
    for (valor, fuente) in entidades {
        conn.execute(
            "INSERT OR IGNORE INTO entidad (entrada_id, valor, fuente) VALUES (?1, ?2, ?3)",
            params![id, valor, fuente.como_str()],
        )?;
    }
    Ok(())
}

/// Recalcula las entidades de todas las entradas.
pub fn recalcular_todas(conn: &Connection, ctx: &Contexto) -> Resultado<()> {
    let ids: Vec<i64> = {
        let mut consulta = conn.prepare("SELECT id FROM entrada")?;
        consulta
            .query_map([], |fila| fila.get(0))?
            .collect::<Result<Vec<i64>, _>>()?
    };
    let tx = crate::db::transaccion_inmediata(conn)?;
    for id in ids {
        recalcular(&tx, id, ctx)?;
    }
    tx.commit()?;
    Ok(())
}

/// Limpia una vez las entidades históricas del repositorio de WI y recalcula las
/// entidades de todas las entradas. Es idempotente mediante una marca en `meta`.
pub fn limpiar_repo_despliegue(conn: &Connection, ajustes: &Ajustes) -> Resultado<()> {
    const MARCA: &str = "limpieza_despliegue_proyectos_v1";
    if db::meta_leer(conn, MARCA)?.is_some() {
        return Ok(());
    }
    let repo = crate::recurso::repo_wi().to_lowercase();
    if repo.is_empty() {
        return Ok(());
    }
    for tabla in ["entidad", "recurso_entidad"] {
        conn.execute(
            &format!(
                "DELETE FROM {tabla}
                  WHERE lower(valor) LIKE 'proyecto:%/' || ?1
                     OR lower(valor) LIKE 'proyecto:%/' || ?1 || '.git'
                     OR lower(valor) = 'app:' || ?1"
            ),
            [&repo],
        )?;
    }
    recalcular_todas(conn, &Contexto::cargar(conn, ajustes)?)?;
    db::meta_escribir(conn, MARCA, "1")?;
    Ok(())
}

/// Recalcula el vocabulario de apps: BD ∪ ajustes ∪ repos git de `proyectosDir`.
/// Devuelve `true` si cambió.
pub fn refrescar_vocabulario(conn: &Connection, ajustes: &Ajustes) -> Resultado<bool> {
    let mut nuevo: BTreeMap<String, &'static str> = BTreeMap::new();

    for app in apps_de_bd(conn)? {
        nuevo.entry(app).or_insert("bd");
    }
    for app in &ajustes.apps {
        let app = app.trim().to_lowercase();
        if !app.is_empty() {
            nuevo.entry(app).or_insert("ajustes");
        }
    }
    for app in apps_de_repos(&ajustes.proyectos_dir, ajustes) {
        nuevo.entry(app).or_insert("repo");
    }

    let actuales: BTreeMap<String, String> = {
        let mut consulta = conn.prepare("SELECT app, fuente FROM vocabulario")?;
        consulta
            .query_map([], |fila| {
                Ok((fila.get::<_, String>(0)?, fila.get::<_, String>(1)?))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?
    };
    if actuales.len() == nuevo.len()
        && actuales
            .iter()
            .all(|(app, fuente)| nuevo.get(app) == Some(&fuente.as_str()))
    {
        return Ok(false);
    }

    let tx = crate::db::transaccion_inmediata(conn)?;
    tx.execute("DELETE FROM vocabulario", [])?;
    for (app, fuente) in &nuevo {
        tx.execute(
            "INSERT INTO vocabulario (app, fuente) VALUES (?1, ?2)",
            params![app, fuente],
        )?;
    }
    tx.commit()?;
    // Las entidades de texto dependen del vocabulario: se rehacen con el nuevo.
    recalcular_todas(conn, &Contexto::cargar(conn, ajustes)?)?;
    Ok(true)
}

fn leer_vocabulario(conn: &Connection) -> Resultado<Vec<String>> {
    let mut consulta = conn.prepare("SELECT app FROM vocabulario ORDER BY app")?;
    Ok(consulta
        .query_map([], |fila| fila.get::<_, String>(0))?
        .collect::<Result<Vec<String>, _>>()?)
}

fn apps_de_bd(conn: &Connection) -> Resultado<Vec<String>> {
    // Solo URLs y WIs aportan apps al vocabulario: lo detectado en texto no se realimenta.
    let mut consulta = conn.prepare(
        "SELECT DISTINCT valor FROM entidad
          WHERE valor LIKE 'app:%' AND fuente = 'url'
         UNION
         SELECT DISTINCT re.valor
           FROM recurso_entidad re
           JOIN recurso r ON r.clave = re.clave
          WHERE re.valor LIKE 'app:%'
            AND r.tipo = 'issue'
            AND ?1 <> ''
            AND lower(r.proyecto) LIKE '%/' || lower(?1)",
    )?;
    let valores = consulta
        .query_map([crate::recurso::repo_wi()], |fila| fila.get::<_, String>(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(valores
        .into_iter()
        .filter_map(|valor| valor.strip_prefix("app:").map(str::to_lowercase))
        .filter(|app| !app.is_empty())
        .collect())
}

/// Apps de los repos git (profundidad 1) cuyo `origin` es del host de GitLab.
fn apps_de_repos(proyectos_dir: &str, ajustes: &Ajustes) -> Vec<String> {
    let carpeta = crate::config::expandir_tilde(proyectos_dir);
    let Ok(entradas) = std::fs::read_dir(&carpeta) else {
        return Vec::new();
    };
    let host = Url::parse(ajustes.gitlab_base_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_lowercase));
    let Some(host) = host else {
        return Vec::new();
    };
    let mut salida: Vec<String> = Vec::new();
    for entrada in entradas.flatten() {
        if !entrada.file_type().is_ok_and(|tipo| tipo.is_dir()) {
            continue;
        }
        if let Some(app) =
            git_remoto(&entrada.path()).and_then(|remoto| app_de_remoto(&remoto, &host))
            && !salida.contains(&app)
        {
            salida.push(app);
        }
    }
    salida
}

/// `git -C <dir> config --get remote.origin.url` sin shell y con 2 s de límite.
fn git_remoto(dir: &Path) -> Option<String> {
    let mut hijo = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--get", "remote.origin.url"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let limite = Instant::now() + Duration::from_secs(2);
    loop {
        match hijo.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= limite {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = hijo.kill();
                let _ = hijo.wait();
                return None;
            }
        }
    }
    let salida = hijo.wait_with_output().ok()?;
    if !salida.status.success() {
        return None;
    }
    let texto = String::from_utf8_lossy(&salida.stdout).trim().to_string();
    if texto.is_empty() { None } else { Some(texto) }
}

/// Nombre del repo a partir del remoto (`git@host:grupo/proy.git` o `https://host/grupo/proy.git`).
fn app_de_remoto(remoto: &str, host_gitlab: &str) -> Option<String> {
    let remoto = remoto.trim();
    let (host, ruta) = if remoto.contains("://") {
        let url = Url::parse(remoto).ok()?;
        (
            url.host_str()?.to_lowercase(),
            url.path().trim_start_matches('/').to_string(),
        )
    } else {
        let (izquierda, derecha) = remoto.split_once(':')?;
        let host = izquierda.rsplit('@').next()?.to_lowercase();
        (host, derecha.to_string())
    };
    if host != host_gitlab {
        return None;
    }
    let ultimo = ruta.trim_end_matches('/').rsplit('/').next()?;
    let ultimo = ultimo.strip_suffix(".git").unwrap_or(ultimo);
    if ultimo.is_empty() {
        None
    } else {
        Some(ultimo.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captura::capturar;
    use crate::db;
    use crate::model::{Ajustes, NuevaEntrada, Origen, TipoEntrada};
    use chrono::{Local, TimeZone};

    #[test]
    fn normaliza_texto_con_acentos_y_separadores() {
        assert_eq!(
            normalizar_texto("Informes-Wéb.Diarios"),
            "informes web diarios"
        );
        assert_eq!(
            normalizar_texto("  GESTIÓN__documental  "),
            "gestion documental"
        );
        assert_eq!(normalizar_texto("diseño/único"), "diseno unico");
        assert_eq!(normalizar_texto(""), "");
    }

    #[test]
    fn apps_con_guiones_y_espacios() {
        let vocabulario = vec!["informes-web".into(), "gestión-documental".into()];
        assert_eq!(
            apps_en_texto("Toco informes web y la gestión documental", &vocabulario),
            vec!["informes-web", "gestión-documental"]
        );
        assert_eq!(
            apps_en_texto("INFORMES-WEB", &vocabulario),
            vec!["informes-web"]
        );
    }

    #[test]
    fn acentos_en_el_texto() {
        let vocabulario = vec!["gestion-documental".into()];
        assert_eq!(
            apps_en_texto("La gestión documental va lenta", &vocabulario),
            vec!["gestion-documental"]
        );
    }

    #[test]
    fn limites_de_palabra() {
        let vocabulario = vec!["notas".into(), "descarga".into()];
        // `notas` no casa con `notas-simples-x`; `descarga` sí es palabra suelta.
        assert!(apps_en_texto("repo notas-simples-x", &vocabulario).is_empty());
        assert_eq!(
            apps_en_texto("las notas de hoy y descarga ya", &vocabulario),
            vec!["notas", "descarga"]
        );
        assert!(apps_en_texto("notasdehoy", &vocabulario).is_empty());
    }

    #[test]
    fn nombres_de_menos_de_cinco_letras_se_ignoran() {
        let vocabulario = vec!["api".into(), "ab".into(), "cinco".into()];
        assert_eq!(
            apps_en_texto("api ab cinco", &vocabulario),
            vec!["cinco"],
            "solo casa el nombre de 5 letras"
        );
        assert!(apps_en_texto("api ab", &vocabulario).is_empty());
    }

    #[test]
    fn app_de_wi_lee_la_seccion() {
        assert_eq!(
            app_de_wi("Contexto\n\n## Aplicación\n\ninformes-web (v1.2.3)\n\nmás"),
            Some("informes-web".into())
        );
        assert_eq!(
            app_de_wi("## Aplicacion\n- app-server\n- [app-client](https://x)"),
            Some("app-server".into())
        );
        assert_eq!(
            app_de_wi("## Aplicación\n\napi-ejemplo (v1.8.1)\n\n¿Cambios? No"),
            Some("api-ejemplo".into())
        );
        assert_eq!(
            app_de_wi("## Aplicación\n\n¿Qué aplicación?\n\notra-cosa"),
            None
        );
        assert_eq!(app_de_wi("## Otra\n\napi-zoho"), None);
        assert_eq!(app_de_wi(""), None);
    }

    #[test]
    fn entidades_de_una_entrada_de_url_y_texto() {
        let ajustes = Ajustes::default();
        let ctx = Contexto {
            prefijos_jira: ["PROJ".to_string()].into_iter().collect(),
            vocabulario: vec!["informes-web".into()],
            ajustes,
        };
        let entrada = EntradaParaEntidades {
            url: Some("https://gitlab.com/equipo/informes-web/-/merge_requests/39".into()),
            titulo: "MR 39 de informes-web, ver PROJ-1896 y FS#45600 y WI 1234".into(),
            seleccion: Some("https://gitlab.com/a/b/-/merge_requests/7".into()),
            notas: vec!["otra nota con PROJ-2272".into()],
        };
        let valores: Vec<String> = de_entrada(&entrada, &ctx)
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        assert!(valores.contains(&"mr:equipo/informes-web!39".into()));
        assert!(valores.contains(&"proyecto:equipo/informes-web".into()));
        assert!(valores.contains(&"app:informes-web".into()));
        assert!(valores.contains(&"jira:PROJ-1896".into()));
        assert!(valores.contains(&"fs:45600".into()));
        assert!(valores.contains(&"wi:1234".into()));
        assert!(valores.contains(&"mr:a/b!7".into()));
        assert!(valores.contains(&"jira:PROJ-2272".into()));
    }

    #[test]
    fn una_wi_aporta_tambien_su_wi() {
        let ctx = Contexto {
            prefijos_jira: HashSet::new(),
            vocabulario: Vec::new(),
            ajustes: Ajustes::default(),
        };
        let entrada = EntradaParaEntidades {
            url: Some("https://gitlab.com/equipo/despliegues/-/work_items/1234".into()),
            titulo: String::new(),
            seleccion: None,
            notas: Vec::new(),
        };
        let valores: Vec<String> = de_entrada(&entrada, &ctx)
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        assert!(valores.contains(&"issue:equipo/despliegues#1234".into()));
        assert!(valores.contains(&"wi:1234".into()));
        assert!(
            !valores
                .iter()
                .any(|valor| valor.starts_with("proyecto:") || valor.starts_with("app:")),
            "el repo de work items no puede aportar proyecto ni app"
        );
    }

    #[test]
    fn limpia_entidades_del_repo_de_despliegue_una_vez() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        conn.execute(
            "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo)
             VALUES ('2026-11-04', '2026-11-04T10:00:00+01:00', '2026-11-04T10:00:00+01:00',
                     'texto', 'cli', 'texto', 'PROJ-1 sin URL')",
            [],
        )
        .expect("entrada");
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO entidad (entrada_id, valor, fuente) VALUES
             (?1, 'proyecto:equipo/despliegues', 'url'),
             (?1, 'app:despliegues', 'url'),
             (?1, 'jira:PROJ-1', 'texto')",
            [id],
        )
        .expect("entidades");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url, proyecto, seguir)
             VALUES ('issue:equipo/despliegues#9', 'issue',
                     'https://x/equipo/despliegues/-/issues/9',
                     'equipo/despliegues', 1)",
            [],
        )
        .expect("recurso");
        conn.execute(
            "INSERT INTO recurso_entidad (clave, valor) VALUES
             ('issue:equipo/despliegues#9',
              'proyecto:equipo/despliegues'),
             ('issue:equipo/despliegues#9',
              'app:despliegues')",
            [],
        )
        .expect("recurso entidad");

        limpiar_repo_despliegue(&conn, &Ajustes::default()).expect("limpia");
        let malas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad
                  WHERE lower(valor) LIKE '%despliegues%'",
                [],
                |fila| fila.get(0),
            )
            .expect("entidad");
        assert_eq!(malas, 0);
        let malas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM recurso_entidad
                  WHERE lower(valor) LIKE '%despliegues%'",
                [],
                |fila| fila.get(0),
            )
            .expect("recurso entidad");
        assert_eq!(malas, 0);
        let buenas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE valor = 'jira:PROJ-1'",
                [],
                |fila| fila.get(0),
            )
            .expect("entidad buena");
        assert_eq!(buenas, 1);
        assert!(
            db::meta_leer(&conn, "limpieza_despliegue_proyectos_v1")
                .expect("meta")
                .is_some()
        );
        // Segunda ejecución idempotente.
        limpiar_repo_despliegue(&conn, &Ajustes::default()).expect("segunda");
    }

    #[test]
    fn recalcula_las_filas_de_entidad() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        conn.execute(
            "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo, seleccion)
             VALUES ('2026-11-04', '2026-11-04T10:00:00+01:00', '2026-11-04T10:00:00+01:00',
                     'texto', 'cli', 'texto', 'PROJ-1896', 'texto')",
            [],
        )
        .expect("entrada");
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO nota (entrada_id, creada, tipo, texto) VALUES (?1, '2026-11-04T10:01:00+01:00', 'nota', 'FS#45600')",
            [id],
        )
        .expect("nota");
        let ctx = Contexto {
            prefijos_jira: ["PROJ".to_string()].into_iter().collect(),
            vocabulario: Vec::new(),
            ajustes: Ajustes::default(),
        };
        recalcular(&conn, id, &ctx).expect("recalcula");
        let valores: Vec<(String, String)> = {
            let mut consulta = conn
                .prepare("SELECT valor, fuente FROM entidad WHERE entrada_id = ?1 ORDER BY valor")
                .expect("consulta");
            consulta
                .query_map([id], |fila| Ok((fila.get(0)?, fila.get(1)?)))
                .expect("filas")
                .collect::<Result<Vec<_>, _>>()
                .expect("valores")
        };
        assert_eq!(
            valores,
            vec![
                ("fs:45600".to_string(), "nota".to_string()),
                ("jira:PROJ-1896".to_string(), "texto".to_string())
            ]
        );
        recalcular(&conn, id, &ctx).expect("recalcula otra vez");
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1",
                [id],
                |f| f.get(0),
            )
            .expect("cuenta");
        assert_eq!(total, 2);
        assert!(recalcular(&conn, 999, &ctx).is_err());
    }

    #[test]
    fn refrescar_vocabulario_detecta_cambios() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        let mut ajustes = Ajustes {
            proyectos_dir: tmp.path().join("proyectos").display().to_string(),
            apps: vec!["desde-ajustes".into()],
            ..Ajustes::default()
        };
        std::fs::create_dir_all(&ajustes.proyectos_dir).expect("carpeta de repos");
        assert!(refrescar_vocabulario(&conn, &ajustes).expect("primera"));
        assert!(!refrescar_vocabulario(&conn, &ajustes).expect("segunda"));

        // Un repo con remoto del host de GitLab añade su app.
        let repo = Path::new(&ajustes.proyectos_dir).join("informes-web");
        std::fs::create_dir_all(&repo).expect("repo");
        let git = |args: &[&str]| {
            let estado = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git");
            assert!(estado.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&[
            "remote",
            "add",
            "origin",
            "git@gitlab.com:equipo/informes-web.git",
        ]);

        assert!(refrescar_vocabulario(&conn, &ajustes).expect("tercera"));
        let apps: Vec<String> = {
            let mut consulta = conn
                .prepare("SELECT app FROM vocabulario ORDER BY app")
                .expect("consulta");
            consulta
                .query_map([], |fila| fila.get(0))
                .expect("filas")
                .collect::<Result<Vec<String>, _>>()
                .expect("apps")
        };
        assert!(apps.contains(&"desde-ajustes".to_string()));
        assert!(apps.contains(&"informes-web".to_string()));

        ajustes.apps.push("otra-app".into());
        assert!(refrescar_vocabulario(&conn, &ajustes).expect("cuarta"));
    }

    #[test]
    fn recalcula_todas_las_entradas() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        for titulo in ["PROJ-1", "PROJ-2"] {
            conn.execute(
                "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo)
                 VALUES ('2026-11-04', '2026-11-04T10:00:00+01:00', '2026-11-04T10:00:00+01:00',
                         'texto', 'cli', 'texto', ?1)",
                [titulo],
            )
            .expect("entrada");
        }
        let ctx = Contexto {
            prefijos_jira: ["PROJ".to_string()].into_iter().collect(),
            vocabulario: Vec::new(),
            ajustes: Ajustes::default(),
        };
        recalcular_todas(&conn, &ctx).expect("recalcula todas");
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entidad WHERE valor LIKE 'jira:%'",
                [],
                |f| f.get(0),
            )
            .expect("cuenta");
        assert_eq!(total, 2);
    }

    #[test]
    fn el_vocabulario_no_se_realimenta_del_texto_y_recalcula_al_cambiar() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        let mut ajustes = Ajustes {
            apps: vec!["app-retirada".into()],
            proyectos_dir: tmp.path().join("proyectos").display().to_string(),
            ..Ajustes::default()
        };
        std::fs::create_dir_all(&ajustes.proyectos_dir).expect("carpeta de repos");
        assert!(refrescar_vocabulario(&conn, &ajustes).expect("primera"));

        let ahora = Local.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap();
        let id = capturar(
            &conn,
            &NuevaEntrada {
                tipo: TipoEntrada::Texto,
                origen: Origen::Global,
                url: None,
                titulo: None,
                seleccion: Some("hablando de app-retirada".into()),
                nota: None,
            },
            &ajustes,
            ahora,
        )
        .expect("captura")
        .id;
        let entidad = |conn: &Connection, id: i64| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM entidad WHERE entrada_id = ?1 AND valor = 'app:app-retirada'",
                [id],
                |fila| fila.get(0),
            )
            .expect("entidad")
        };
        assert_eq!(entidad(&conn, id), 1);

        // Fuera de ajustes: no puede quedarse en el vocabulario solo porque el texto la mencione.
        ajustes.apps.clear();
        assert!(refrescar_vocabulario(&conn, &ajustes).expect("segunda"));
        let en_vocabulario: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vocabulario WHERE app = 'app-retirada'",
                [],
                |fila| fila.get(0),
            )
            .expect("vocabulario");
        assert_eq!(en_vocabulario, 0);
        assert_eq!(
            entidad(&conn, id),
            0,
            "recalcular_todas limpia la entidad de texto"
        );
    }
}
