//! Seguimiento de recursos remotos: selección, consulta, huella, diff y planificador.

pub mod freshservice;
pub mod gitlab;
pub mod jira;

use crate::config::Credenciales;
use crate::db;
use crate::model::{Ajustes, Cambio, Comentario, EstadoSeguimiento, Huella};
use crate::{captura, entidades, extraer, huella, recurso, util};
use chrono::{DateTime, Duration, Local};
use futures::stream::{self, StreamExt};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc;

/// Lo que devuelve una fuente para un recurso.
#[derive(Debug, Clone)]
pub struct Remoto {
    pub huella: Huella,
    /// Título + descripción, para extraer referencias.
    pub texto: String,
    pub titulo: Option<String>,
    pub estado: Option<String>,
    pub app: Option<String>,
    /// Fallo de identidad de la fuente que no impide leer el recurso (p. ej. FS sin `/me`).
    pub identidad_error: Option<String>,
}

/// Error de una fuente. Nunca lleva cuerpos ni credenciales.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorFuente {
    NoExiste,
    SinCredenciales,
    Http(u16),
    Red(String),
}

impl ErrorFuente {
    /// Texto corto para `recurso.error` y `EstadoSeguimiento.errores`.
    pub fn resumen(&self, fuente: &str) -> String {
        match self {
            Self::NoExiste => "No existe o sin acceso".to_string(),
            Self::SinCredenciales => format!("Sin credenciales de {fuente}"),
            Self::Http(codigo) => format!("{fuente} respondió {codigo}"),
            Self::Red(texto) if texto.trim().is_empty() => {
                format!("Error de red con {fuente}")
            }
            Self::Red(texto) => texto.clone(),
        }
    }

    /// Añade contexto a un fallo de identidad: «Freshservice respondió 403 al leer tu agente».
    pub fn contextual(self, fuente: &str, contexto: &str) -> Self {
        match self {
            Self::Http(codigo) => Self::Red(format!("{fuente} respondió {codigo} {contexto}")),
            Self::Red(texto) => Self::Red(format!("{texto} {contexto}")),
            otro => otro,
        }
    }
}

#[derive(Default)]
struct EstadoIdentidad<T> {
    exito: Option<T>,
    fallo: Option<ErrorFuente>,
    vuelta_fallo: u64,
}

/// Identidad del usuario en cada fuente. Los éxitos se conservan durante todo el proceso;
/// los fallos solo durante la vuelta en la que ocurrieron (se reintentan en la siguiente).
#[derive(Default)]
pub struct Identidad {
    vuelta: std::sync::atomic::AtomicU64,
    jira: tokio::sync::Mutex<EstadoIdentidad<String>>,
    gitlab: tokio::sync::Mutex<EstadoIdentidad<String>>,
    fs: tokio::sync::Mutex<EstadoIdentidad<u64>>,
}

impl Identidad {
    /// Marca el comienzo de una vuelta: los fallos de identidad dejan de recordarse.
    pub fn nueva_vuelta(&self) {
        self.vuelta
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn generacion(&self) -> u64 {
        self.vuelta.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// `accountId` de Jira.
    pub(crate) async fn cuenta_jira(
        &self,
        cli: &reqwest::Client,
        aj: &Ajustes,
        cred: &Credenciales,
    ) -> Result<String, ErrorFuente> {
        let generacion = self.generacion();
        let mut estado = self.jira.lock().await;
        if let Some(cuenta) = estado.exito.as_ref() {
            return Ok(cuenta.clone());
        }
        if estado.vuelta_fallo == generacion
            && let Some(error) = estado.fallo.as_ref()
        {
            return Err(error.clone());
        }
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
        let respuesta = match cli
            .get(format!("{base}/rest/api/3/myself"))
            .basic_auth(usuario, Some(token))
            .send()
            .await
        {
            Ok(respuesta) => respuesta,
            Err(error) => {
                return Err(recordar(&mut estado, generacion, error_red("Jira", &error)));
            }
        };
        if !respuesta.status().is_success() {
            return Err(recordar(
                &mut estado,
                generacion,
                error_http(respuesta.status()),
            ));
        }
        let cuerpo: Value = match respuesta.json().await {
            Ok(cuerpo) => cuerpo,
            Err(_) => {
                return Err(recordar(
                    &mut estado,
                    generacion,
                    ErrorFuente::Red("Respuesta ilegible de Jira".into()),
                ));
            }
        };
        let Some(cuenta) = opcional(&cuerpo, &["accountId"]) else {
            return Err(recordar(
                &mut estado,
                generacion,
                ErrorFuente::Red("Jira no devolvió la cuenta".into()),
            ));
        };
        estado.exito = Some(cuenta.clone());
        estado.fallo = None;
        Ok(cuenta)
    }

    /// `username` de GitLab.
    pub(crate) async fn usuario_gitlab(
        &self,
        cli: &reqwest::Client,
        aj: &Ajustes,
        cred: &Credenciales,
    ) -> Result<String, ErrorFuente> {
        let generacion = self.generacion();
        let mut estado = self.gitlab.lock().await;
        if let Some(usuario) = estado.exito.as_ref() {
            return Ok(usuario.clone());
        }
        if estado.vuelta_fallo == generacion
            && let Some(error) = estado.fallo.as_ref()
        {
            return Err(error.clone());
        }
        let token = token_gitlab(cred)?;
        let base = format!("{}/api/v4", aj.gitlab_base_url.trim_end_matches('/'));
        let respuesta = match cli
            .get(format!("{base}/user"))
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
        {
            Ok(respuesta) => respuesta,
            Err(error) => {
                return Err(recordar(
                    &mut estado,
                    generacion,
                    error_red("GitLab", &error),
                ));
            }
        };
        if !respuesta.status().is_success() {
            return Err(recordar(
                &mut estado,
                generacion,
                error_http(respuesta.status()),
            ));
        }
        let cuerpo: Value = match respuesta.json().await {
            Ok(cuerpo) => cuerpo,
            Err(_) => {
                return Err(recordar(
                    &mut estado,
                    generacion,
                    ErrorFuente::Red("Respuesta ilegible de GitLab".into()),
                ));
            }
        };
        let Some(usuario) = opcional(&cuerpo, &["username"]) else {
            return Err(recordar(
                &mut estado,
                generacion,
                ErrorFuente::Red("GitLab no devolvió el usuario".into()),
            ));
        };
        estado.exito = Some(usuario.clone());
        estado.fallo = None;
        Ok(usuario)
    }

    /// Id del agente dueño de la clave de Freshservice (`/agents/me`).
    pub(crate) async fn agente_fs(
        &self,
        cli: &reqwest::Client,
        aj: &Ajustes,
        cred: &Credenciales,
    ) -> Result<u64, ErrorFuente> {
        let generacion = self.generacion();
        let mut estado = self.fs.lock().await;
        if let Some(agente) = estado.exito {
            return Ok(agente);
        }
        if estado.vuelta_fallo == generacion
            && let Some(error) = estado.fallo.as_ref()
        {
            return Err(error.clone());
        }
        let clave = cred
            .fs_key
            .as_deref()
            .filter(|valor| !valor.is_empty())
            .ok_or(ErrorFuente::SinCredenciales)?;
        let base = aj.fs_base_url.trim_end_matches('/');
        let respuesta = match cli
            .get(format!("{base}/api/v2/agents/me"))
            .basic_auth(clave, Some("X"))
            .send()
            .await
        {
            Ok(respuesta) => respuesta,
            Err(error) => {
                return Err(recordar(
                    &mut estado,
                    generacion,
                    error_red("Freshservice", &error),
                ));
            }
        };
        if !respuesta.status().is_success() {
            return Err(recordar(
                &mut estado,
                generacion,
                error_http(respuesta.status()),
            ));
        }
        let cuerpo: Value = match respuesta.json().await {
            Ok(cuerpo) => cuerpo,
            Err(_) => {
                return Err(recordar(
                    &mut estado,
                    generacion,
                    ErrorFuente::Red("Respuesta ilegible de Freshservice".into()),
                ));
            }
        };
        let agente = match cuerpo
            .get("agent")
            .unwrap_or(&cuerpo)
            .get("id")
            .and_then(Value::as_u64)
        {
            Some(agente) => agente,
            None => {
                return Err(recordar(
                    &mut estado,
                    generacion,
                    ErrorFuente::Red("Freshservice no devolvió el agente".into()),
                ));
            }
        };
        estado.exito = Some(agente);
        estado.fallo = None;
        Ok(agente)
    }
}

fn recordar<T>(
    estado: &mut EstadoIdentidad<T>,
    generacion: u64,
    error: ErrorFuente,
) -> ErrorFuente {
    estado.fallo = Some(error.clone());
    estado.vuelta_fallo = generacion;
    error
}

/// Resultado de una vuelta de seguimiento.
#[derive(Debug, Clone, Default)]
pub struct InformeVuelta {
    pub seguidos: u32,
    pub errores: Vec<String>,
    pub avisos: Vec<Aviso>,
}

/// Aviso de escritorio para un recurso con novedades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aviso {
    pub clave: String,
    pub titulo: String,
    pub cuerpo: String,
}

/// Selección y consulta de todos los recursos que toca mirar.
pub async fn vuelta(
    ruta_bd: &Path,
    cred: &Credenciales,
    aj: &Ajustes,
    ident: &Identidad,
    solo_sin_huella: bool,
    ahora: DateTime<Local>,
) -> InformeVuelta {
    ident.nueva_vuelta();
    let recursos = match seleccionar(ruta_bd, aj, solo_sin_huella, ahora) {
        Ok(recursos) => recursos,
        Err(error) => {
            return InformeVuelta {
                errores: vec![error.to_string()],
                ..InformeVuelta::default()
            };
        }
    };
    let mut informe = InformeVuelta::default();
    if recursos.is_empty() {
        return informe;
    }

    let mut jira = Vec::new();
    let mut gitlab = Vec::new();
    let mut fs = Vec::new();
    for recurso in recursos {
        match recurso.tipo.as_str() {
            "jira" => jira.push(recurso),
            "mr" | "issue" => gitlab.push(recurso),
            "fs" => fs.push(recurso),
            _ => {}
        }
    }

    let cli = crate::http::cliente();
    let ctx = Contexto {
        ruta: ruta_bd,
        cred,
        aj,
        ident,
        cli: &cli,
    };
    for (fuente, lote, configurada) in [
        (FuenteRecurso::Jira, jira, tiene_jira(cred)),
        (FuenteRecurso::Gitlab, gitlab, tiene_gitlab(cred)),
        (FuenteRecurso::Fs, fs, tiene_fs(cred)),
    ] {
        if lote.is_empty() {
            continue;
        }
        if !configurada {
            let texto = format!("Sin credenciales de {}", fuente.nombre());
            if !informe.errores.contains(&texto) {
                informe.errores.push(texto);
            }
            continue;
        }
        informe.seguidos += u32::try_from(lote.len()).unwrap_or(u32::MAX);
        let resultados = grupo(fuente, &ctx, lote, ahora).await;
        for resultado in resultados {
            if let Some(error) = resultado.error
                && !informe.errores.contains(&error)
            {
                informe.errores.push(error);
            }
            if let Some(aviso) = resultado.aviso {
                informe.avisos.push(aviso);
            }
        }
    }
    informe.avisos.sort_by(|a, b| a.clave.cmp(&b.clave));
    informe
}

/// Planificador periódico en un hilo propio.
pub struct Planificador;

impl Planificador {
    /// Arranca el hilo de seguimiento. Primera vuelta a los 5 s y luego cada
    /// `intervaloMin` (mínimo 1 minuto).
    pub fn arrancar(
        ruta_bd: impl Into<PathBuf>,
        cred: Credenciales,
        ajustes: Arc<RwLock<Ajustes>>,
        al_estado: impl Fn(EstadoSeguimiento) + Send + Sync + 'static,
    ) -> ManejadorSeguimiento {
        arrancar(ruta_bd, cred, ajustes, al_estado)
    }
}

/// Igual que [`Planificador::arrancar`], como función libre.
pub fn arrancar(
    ruta_bd: impl Into<PathBuf>,
    cred: Credenciales,
    ajustes: Arc<RwLock<Ajustes>>,
    al_estado: impl Fn(EstadoSeguimiento) + Send + Sync + 'static,
) -> ManejadorSeguimiento {
    let entorno = Entorno {
        ruta: ruta_bd.into(),
        cred,
        ajustes,
    };
    let (ordenes, receptor) = mpsc::unbounded_channel();
    let estado = Arc::new(Mutex::new(EstadoSeguimiento {
        proxima: Some((Local::now() + Duration::seconds(5)).to_rfc3339()),
        ..EstadoSeguimiento::default()
    }));
    let estado_hilo = Arc::clone(&estado);

    let hilo = std::thread::Builder::new()
        .name("migas-seguimiento".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("migas seguimiento: no se pudo crear el runtime: {error}");
                    return;
                }
            };
            runtime.block_on(bucle(entorno, al_estado, estado_hilo, receptor));
        });
    if let Err(error) = hilo {
        eprintln!("migas seguimiento: no se pudo arrancar el hilo: {error}");
    }

    ManejadorSeguimiento { ordenes, estado }
}

/// Mando del planificador: lanzar vueltas y consultar el estado.
#[derive(Clone)]
pub struct ManejadorSeguimiento {
    ordenes: mpsc::UnboundedSender<Orden>,
    estado: Arc<Mutex<EstadoSeguimiento>>,
}

impl ManejadorSeguimiento {
    /// Vuelta completa inmediata. Si ya hay una en curso no lanza otra.
    pub fn comprobar_ahora(&self) {
        if self.estado().en_curso {
            return;
        }
        let _ = self.ordenes.send(Orden::Completa);
    }

    /// Vuelta rápida de los recursos sin huella (como mucho una cada 3 s).
    pub fn pendientes(&self) {
        let _ = self.ordenes.send(Orden::Pendientes);
    }

    /// Estado actual del seguimiento.
    pub fn estado(&self) -> EstadoSeguimiento {
        match self.estado.lock() {
            Ok(guardia) => guardia.clone(),
            Err(veneno) => veneno.into_inner().clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Orden {
    Completa,
    Pendientes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FuenteRecurso {
    Jira,
    Gitlab,
    Fs,
}

impl FuenteRecurso {
    fn nombre(self) -> &'static str {
        match self {
            Self::Jira => "Jira",
            Self::Gitlab => "GitLab",
            Self::Fs => "Freshservice",
        }
    }
}

/// Recurso que toca consultar en esta vuelta.
#[derive(Debug, Clone)]
struct Seguido {
    clave: String,
    tipo: String,
    proyecto: Option<String>,
}

/// Entorno compartido por las tareas de una vuelta.
struct Contexto<'a> {
    ruta: &'a Path,
    cred: &'a Credenciales,
    aj: &'a Ajustes,
    ident: &'a Identidad,
    cli: &'a reqwest::Client,
}

struct ResultadoRecurso {
    error: Option<String>,
    aviso: Option<Aviso>,
}

/// Entorno del planificador (debe vivir en el hilo mientras dure).
struct Entorno {
    ruta: PathBuf,
    cred: Credenciales,
    ajustes: Arc<RwLock<Ajustes>>,
}

async fn bucle(
    entorno: Entorno,
    al_estado: impl Fn(EstadoSeguimiento),
    estado: Arc<Mutex<EstadoSeguimiento>>,
    mut ordenes: mpsc::UnboundedReceiver<Orden>,
) {
    let ident = Identidad::default();
    let mut proxima = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut ultimo_pendientes: Option<tokio::time::Instant> = None;
    let mut pedido_completo = false;
    let mut pedido_pendientes = false;

    loop {
        // Agrupa lo que haya llegado mientras se trabajaba.
        while let Ok(orden) = ordenes.try_recv() {
            match orden {
                Orden::Completa => pedido_completo = true,
                Orden::Pendientes => pedido_pendientes = true,
            }
        }

        let ahora = tokio::time::Instant::now();
        let toca_periodica = ahora >= proxima;
        let pendiente_permitido = ultimo_pendientes.is_none_or(|instante| {
            ahora.duration_since(instante) >= std::time::Duration::from_secs(3)
        });
        if pedido_completo || toca_periodica || (pedido_pendientes && pendiente_permitido) {
            let completa = pedido_completo || toca_periodica;
            if completa {
                pedido_completo = false;
                pedido_pendientes = false;
            } else {
                pedido_pendientes = false;
                ultimo_pendientes = Some(ahora);
            }
            let ajustes = leer_ajustes(&entorno.ajustes);
            ejecutar_vuelta(
                &entorno,
                &ident,
                !completa,
                Local::now(),
                &estado,
                &al_estado,
            )
            .await;
            // Solo la vuelta completa reinicia el periodo; las rápidas no lo retrasan.
            if completa {
                let intervalo =
                    std::time::Duration::from_secs(u64::from(ajustes.intervalo_min.max(1)) * 60);
                proxima = tokio::time::Instant::now() + intervalo;
            }
            continue;
        }

        let espera_periodica = tokio::time::sleep_until(proxima);
        let espera_pendiente = async {
            if pedido_pendientes {
                let base = ultimo_pendientes.unwrap_or(ahora);
                tokio::time::sleep_until(base + std::time::Duration::from_secs(3)).await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            orden = ordenes.recv() => match orden {
                Some(Orden::Completa) => pedido_completo = true,
                Some(Orden::Pendientes) => pedido_pendientes = true,
                None => break,
            },
            () = espera_periodica => {}
            () = espera_pendiente => {}
        }
    }
}

async fn ejecutar_vuelta(
    entorno: &Entorno,
    ident: &Identidad,
    solo_sin_huella: bool,
    ahora: DateTime<Local>,
    estado: &Arc<Mutex<EstadoSeguimiento>>,
    al_estado: &impl Fn(EstadoSeguimiento),
) {
    {
        let mut guardia = match estado.lock() {
            Ok(guardia) => guardia,
            Err(veneno) => veneno.into_inner(),
        };
        guardia.en_curso = true;
    }
    publicar(estado, al_estado);

    let ajustes = leer_ajustes(&entorno.ajustes);
    let informe = vuelta(
        &entorno.ruta,
        &entorno.cred,
        &ajustes,
        ident,
        solo_sin_huella,
        ahora,
    )
    .await;

    if ajustes.notificar {
        if informe.avisos.len() > 3 {
            crate::notificar::notificar(
                "migas",
                &format!("{} elementos con cambios", informe.avisos.len()),
            );
        } else {
            for aviso in &informe.avisos {
                crate::notificar::notificar(&aviso.titulo, &aviso.cuerpo);
            }
        }
    }

    let fin = Local::now();
    let proxima = fin + Duration::minutes(i64::from(ajustes.intervalo_min.max(1)));
    {
        let mut guardia = match estado.lock() {
            Ok(guardia) => guardia,
            Err(veneno) => veneno.into_inner(),
        };
        guardia.en_curso = false;
        guardia.ultima = Some(fin.to_rfc3339());
        // En una vuelta rápida no se toca el periodo de la completa.
        if !solo_sin_huella {
            guardia.proxima = Some(proxima.to_rfc3339());
        }
        guardia.seguidos = informe.seguidos;
        guardia.errores = informe.errores;
    }
    publicar(estado, al_estado);
}

fn publicar(estado: &Arc<Mutex<EstadoSeguimiento>>, al_estado: &impl Fn(EstadoSeguimiento)) {
    let copia = match estado.lock() {
        Ok(guardia) => guardia.clone(),
        Err(veneno) => veneno.into_inner().clone(),
    };
    al_estado(copia);
}

fn leer_ajustes(ajustes: &Arc<RwLock<Ajustes>>) -> Ajustes {
    ajustes
        .read()
        .map(|guardia| guardia.clone())
        .unwrap_or_default()
}

async fn grupo(
    fuente: FuenteRecurso,
    ctx: &Contexto<'_>,
    recursos: Vec<Seguido>,
    ahora: DateTime<Local>,
) -> Vec<ResultadoRecurso> {
    stream::iter(recursos)
        .map(|fila| async move {
            let consulta = match fuente {
                FuenteRecurso::Jira => {
                    jira::consultar(ctx.cli, ctx.aj, ctx.cred, ctx.ident, &fila.clave).await
                }
                FuenteRecurso::Gitlab => {
                    gitlab::consultar(ctx.cli, ctx.aj, ctx.cred, ctx.ident, &fila.clave).await
                }
                FuenteRecurso::Fs => {
                    freshservice::consultar(ctx.cli, ctx.aj, ctx.cred, ctx.ident, &fila.clave).await
                }
            };
            match consulta {
                Ok(remoto) => {
                    let identidad_error = remoto.identidad_error.clone();
                    match guardar(ctx.ruta, &fila, remoto, ahora, ctx.aj) {
                        Ok(aviso) => ResultadoRecurso {
                            error: identidad_error,
                            aviso,
                        },
                        Err(error) => ResultadoRecurso {
                            error: Some(error.to_string()),
                            aviso: None,
                        },
                    }
                }
                Err(error) => {
                    let resumen = error.resumen(fuente.nombre());
                    let _ = guardar_error(ctx.ruta, &fila.clave, &error, fuente.nombre());
                    ResultadoRecurso {
                        error: Some(resumen),
                        aviso: None,
                    }
                }
            }
        })
        .buffer_unordered(4)
        .collect()
        .await
}

fn seleccionar(
    ruta_bd: &Path,
    aj: &Ajustes,
    solo_sin_huella: bool,
    ahora: DateTime<Local>,
) -> crate::error::Resultado<Vec<Seguido>> {
    let conn = db::abrir(ruta_bd)?;
    let desde = (ahora - Duration::days(i64::from(aj.dias_seguimiento))).to_rfc3339();
    let filtro = if solo_sin_huella {
        "AND r.huella_actual IS NULL AND r.error IS NULL"
    } else {
        ""
    };
    let sql = format!(
        "SELECT r.clave, r.tipo, r.proyecto
           FROM recurso r
          WHERE r.seguir = 1
            AND (
                EXISTS (SELECT 1 FROM entrada e
                         WHERE e.recurso = r.clave AND e.borrada IS NULL
                           AND julianday(e.creada) >= julianday(?1))
                OR EXISTS (SELECT 1 FROM entrada e
                            WHERE e.recurso = r.clave AND e.borrada IS NULL AND e.fijada = 1)
            )
            {filtro}
          ORDER BY r.clave"
    );
    let mut consulta = conn.prepare(&sql)?;
    let filas = consulta.query_map([desde], |fila| {
        Ok(Seguido {
            clave: fila.get(0)?,
            tipo: fila.get(1)?,
            proyecto: fila.get(2)?,
        })
    })?;
    Ok(filas.collect::<Result<Vec<Seguido>, _>>()?)
}

struct EstadoPrevio {
    huella_vista: Option<String>,
    pendiente_visto: i64,
    cambios: Option<String>,
}

fn guardar(
    ruta_bd: &Path,
    fila: &Seguido,
    mut remoto: Remoto,
    ahora: DateTime<Local>,
    aj: &Ajustes,
) -> crate::error::Resultado<Option<Aviso>> {
    remoto.huella.tomada = ahora.to_rfc3339();
    let conn = db::abrir(ruta_bd)?;
    let tx = db::transaccion_inmediata(&conn)?;
    let previo: Option<EstadoPrevio> = tx
        .query_row(
            "SELECT huella_vista, pendiente_visto, cambios FROM recurso WHERE clave = ?1",
            [&fila.clave],
            |fila| {
                Ok(EstadoPrevio {
                    huella_vista: fila.get(0)?,
                    pendiente_visto: fila.get(1)?,
                    cambios: fila.get(2)?,
                })
            },
        )
        .optional()?;
    let Some(previo) = previo else {
        return Ok(None);
    };

    let vista_existente: Option<Huella> = previo
        .huella_vista
        .as_deref()
        .and_then(|texto| serde_json::from_str(texto).ok());
    let vista = vista_existente.clone().unwrap_or_default();
    let copiar = vista_existente.is_none() || previo.pendiente_visto != 0;

    let mut cambios: Vec<Cambio> = Vec::new();
    let vista_nueva = if copiar {
        Some(remoto.huella.clone())
    } else {
        cambios = huella::cambios(&vista, &remoto.huella);
        if huella::solo_mios(&vista, &remoto.huella) {
            cambios.clear();
            Some(remoto.huella.clone())
        } else {
            None
        }
    };

    let cambios_previos: Vec<Cambio> = previo
        .cambios
        .as_deref()
        .map(|texto| serde_json::from_str(texto).unwrap_or_default())
        .unwrap_or_default();
    let comentarios_previos = n_comentarios(&cambios_previos);
    let comentarios_nuevos = n_comentarios(&cambios);
    let aviso = if (cambios_previos.is_empty() && !cambios.is_empty())
        || comentarios_nuevos > comentarios_previos
    {
        Some(Aviso {
            clave: fila.clave.clone(),
            titulo: format!("migas · {}", recurso::visible(&fila.clave)),
            cuerpo: resumen_cambios(&cambios),
        })
    } else {
        None
    };

    let huella_actual = serde_json::to_string(&remoto.huella)?;
    let huella_vista = vista_nueva
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let cambios_json = if cambios.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&cambios)?)
    };
    tx.execute(
        "UPDATE recurso
            SET titulo = ?2, estado = ?3, app = ?4, huella_actual = ?5, comprobado = ?6,
                error = NULL, huella_vista = COALESCE(?7, huella_vista),
                pendiente_visto = 0, cambios = ?8
          WHERE clave = ?1",
        params![
            fila.clave,
            remoto.titulo,
            remoto.estado,
            remoto.app,
            huella_actual,
            ahora.to_rfc3339(),
            huella_vista,
            cambios_json,
        ],
    )?;

    escribir_entidades(&tx, fila, &remoto, aj)?;
    actualizar_titulos_por_defecto(&tx, fila, &remoto)?;

    let entradas: Vec<i64> = {
        let mut consulta = tx
            .prepare("SELECT id FROM entrada WHERE recurso = ?1 AND borrada IS NULL ORDER BY id")?;
        consulta
            .query_map([&fila.clave], |fila| fila.get(0))?
            .collect::<Result<Vec<i64>, _>>()?
    };
    for id in entradas {
        captura::reindexar(&tx, id)?;
    }
    tx.commit()?;
    Ok(aviso)
}

/// Si la entrada se guardó con el host como título por defecto, usa el título remoto.
/// El bucle posterior de `reindexar` actualiza el índice FTS.
fn actualizar_titulos_por_defecto(
    conn: &rusqlite::Connection,
    fila: &Seguido,
    remoto: &Remoto,
) -> crate::error::Resultado<()> {
    let Some(titulo_remoto) = remoto
        .titulo
        .as_deref()
        .map(str::trim)
        .filter(|titulo| !titulo.is_empty())
    else {
        return Ok(());
    };
    let mut consulta = conn.prepare(
        "SELECT id, url, titulo FROM entrada
          WHERE recurso = ?1 AND borrada IS NULL ORDER BY id",
    )?;
    let filas = consulta
        .query_map([&fila.clave], |fila| {
            Ok((
                fila.get::<_, i64>(0)?,
                fila.get::<_, Option<String>>(1)?,
                fila.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(consulta);
    for (id, url, titulo_actual) in filas {
        let Some(host) = url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
            .and_then(|url| url.host_str().map(str::to_string))
        else {
            continue;
        };
        if titulo_actual.trim() == host {
            conn.execute(
                "UPDATE entrada SET titulo = ?2 WHERE id = ?1",
                params![id, titulo_remoto],
            )?;
        }
    }
    Ok(())
}

fn escribir_entidades(
    conn: &rusqlite::Connection,
    fila: &Seguido,
    remoto: &Remoto,
    aj: &Ajustes,
) -> crate::error::Resultado<()> {
    conn.execute(
        "DELETE FROM recurso_entidad WHERE clave = ?1",
        [&fila.clave],
    )?;
    let ctx = entidades::Contexto::cargar(conn, aj)?;
    let mut valores: BTreeSet<String> = BTreeSet::new();
    for referencia in extraer::extraer(&remoto.texto, &ctx.prefijos_jira) {
        valores.insert(entidades::valor_de_ref(&referencia));
    }
    if let Some(app) = remoto
        .app
        .as_deref()
        .filter(|app| !app.is_empty())
        .filter(|app| !recurso::es_repo_wi(app))
    {
        valores.insert(format!("app:{app}"));
    }
    if let Some(proyecto) = fila
        .proyecto
        .as_deref()
        .filter(|proyecto| !proyecto.is_empty())
        && matches!(fila.tipo.as_str(), "mr" | "issue")
    {
        if !recurso::es_repo_wi(proyecto) {
            valores.insert(format!("proyecto:{proyecto}"));
            if let Some(app) = recurso::app_de_ruta(proyecto) {
                valores.insert(format!("app:{app}"));
            }
        }
        if fila.tipo == "issue"
            && recurso::es_repo_wi(proyecto)
            && let Some((_, numero)) = fila.clave.rsplit_once('#')
        {
            valores.insert(format!("wi:{numero}"));
        }
    }
    for valor in valores {
        conn.execute(
            "INSERT OR IGNORE INTO recurso_entidad (clave, valor) VALUES (?1, ?2)",
            params![fila.clave, valor],
        )?;
    }
    Ok(())
}

fn guardar_error(
    ruta_bd: &Path,
    clave: &str,
    error: &ErrorFuente,
    fuente: &str,
) -> crate::error::Resultado<()> {
    let conn = db::abrir(ruta_bd)?;
    match error {
        ErrorFuente::NoExiste => {
            conn.execute(
                "UPDATE recurso SET error = 'No existe o sin acceso', seguir = 0 WHERE clave = ?1",
                [clave],
            )?;
        }
        _ => {
            conn.execute(
                "UPDATE recurso SET error = ?2 WHERE clave = ?1",
                params![clave, error.resumen(fuente)],
            )?;
        }
    }
    Ok(())
}

fn n_comentarios(cambios: &[Cambio]) -> u32 {
    cambios
        .iter()
        .filter_map(|cambio| match cambio {
            Cambio::Comentarios { n, .. } => Some(*n),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

fn resumen_cambios(cambios: &[Cambio]) -> String {
    let partes: Vec<String> = cambios
        .iter()
        .take(2)
        .map(|cambio| match cambio {
            Cambio::Comentarios { n, autores, .. } => match autores.as_slice() {
                [autor] => format!("{autor} comentó"),
                _ if *n == 1 => "1 comentario".to_string(),
                _ => format!("{n} comentarios"),
            },
            Cambio::Estado { antes, despues } => format!(
                "Estado: {} → {}",
                antes.as_deref().unwrap_or("sin estado"),
                despues.as_deref().unwrap_or("sin estado")
            ),
            Cambio::Asignado { antes, despues } => format!(
                "Asignado: {} → {}",
                antes.as_deref().unwrap_or("nadie"),
                despues.as_deref().unwrap_or("nadie")
            ),
            Cambio::Titulo { .. } => "Título cambiado".to_string(),
            Cambio::Pipeline { antes, despues } => format!(
                "Pipeline: {} → {}",
                antes.as_deref().unwrap_or("sin pipeline"),
                despues.as_deref().unwrap_or("sin pipeline")
            ),
            Cambio::Fusionada => "Fusionada".to_string(),
            Cambio::Etiquetas { anadidas, quitadas } => {
                let mut partes = Vec::new();
                if !anadidas.is_empty() {
                    partes.push(format!("+{}", anadidas.join(", ")));
                }
                if !quitadas.is_empty() {
                    partes.push(format!("-{}", quitadas.join(", ")));
                }
                format!("Etiquetas: {}", partes.join(" "))
            }
        })
        .collect();
    partes.join(" · ")
}

fn tiene_jira(cred: &Credenciales) -> bool {
    cred.jira_user.as_deref().is_some_and(|v| !v.is_empty())
        && cred.jira_token.as_deref().is_some_and(|v| !v.is_empty())
}

fn tiene_gitlab(cred: &Credenciales) -> bool {
    cred.gitlab_token.as_deref().is_some_and(|v| !v.is_empty())
}

fn tiene_fs(cred: &Credenciales) -> bool {
    cred.fs_key.as_deref().is_some_and(|v| !v.is_empty())
}

fn token_gitlab(cred: &Credenciales) -> Result<&str, ErrorFuente> {
    cred.gitlab_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .ok_or(ErrorFuente::SinCredenciales)
}

fn error_http(status: reqwest::StatusCode) -> ErrorFuente {
    ErrorFuente::Http(status.as_u16())
}

fn error_red(fuente: &str, error: &reqwest::Error) -> ErrorFuente {
    let texto = if error.is_timeout() {
        format!("Timeout de {fuente}")
    } else if error.is_connect() {
        format!("No se pudo conectar con {fuente}")
    } else if error.is_decode() {
        format!("Respuesta ilegible de {fuente}")
    } else {
        format!("Error de red con {fuente}")
    };
    ErrorFuente::Red(texto)
}

pub(crate) fn cadena(valor: &Value, ruta: &[&str]) -> String {
    opcional(valor, ruta).unwrap_or_default()
}

pub(crate) fn opcional(valor: &Value, ruta: &[&str]) -> Option<String> {
    let mut actual = valor;
    for paso in ruta {
        actual = actual.get(*paso)?;
    }
    actual
        .as_str()
        .map(str::trim)
        .filter(|texto| !texto.is_empty())
        .map(str::to_string)
}

pub(crate) fn texto_doc(valor: Option<&Value>) -> String {
    match valor {
        Some(Value::String(texto)) => texto.clone(),
        Some(valor @ Value::Object(_)) => crate::adf::adf_a_texto(valor),
        _ => String::new(),
    }
}

/// Una línea de espacios colapsados, como mucho 240 caracteres por `char`.
pub(crate) fn extracto(texto: &str) -> String {
    texto
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

/// Fecha normalizada a RFC 3339; si no parsea se conserva tal cual.
pub(crate) fn fecha(valor: &str) -> String {
    util::parse_fecha(valor)
        .map(|fecha| fecha.to_rfc3339())
        .unwrap_or_else(|| valor.to_string())
}

/// Id de un comentario, venga como texto o como número.
pub(crate) fn id_texto(valor: &Value) -> String {
    match valor.get("id") {
        Some(Value::String(texto)) => texto.clone(),
        Some(Value::Number(numero)) => numero.to_string(),
        _ => String::new(),
    }
}

/// Ordena por fecha ascendente y devuelve los 10 más recientes, del más nuevo al más viejo.
pub(crate) fn ultimos_comentarios(mut comentarios: Vec<Comentario>) -> Vec<Comentario> {
    comentarios.sort_by_key(|comentario| {
        util::parse_fecha(&comentario.fecha)
            .map(|fecha| fecha.timestamp_millis())
            .unwrap_or(i64::MIN)
    });
    let desde = comentarios.len().saturating_sub(10);
    let mut ultimos = comentarios.split_off(desde);
    ultimos.reverse();
    ultimos
}

/// Escapa una ruta de proyecto para un segmento de URL (`grupo%2Fproy`).
pub(crate) fn codificar(ruta: &str) -> String {
    let mut salida = String::with_capacity(ruta.len());
    for byte in ruta.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                salida.push(byte as char);
            }
            _ => salida.push_str(&format!("%{byte:02X}")),
        }
    }
    salida
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use httpmock::prelude::*;

    fn cuando(dia: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, dia, 10, 0, 0).unwrap()
    }

    #[test]
    fn resumenes_de_error_sin_cuerpos() {
        assert_eq!(ErrorFuente::Http(401).resumen("Jira"), "Jira respondió 401");
        assert_eq!(
            ErrorFuente::Red("Timeout de GitLab".into()).resumen("GitLab"),
            "Timeout de GitLab"
        );
        assert_eq!(
            ErrorFuente::SinCredenciales.resumen("Freshservice"),
            "Sin credenciales de Freshservice"
        );
        assert_eq!(
            ErrorFuente::NoExiste.resumen("Jira"),
            "No existe o sin acceso"
        );
    }

    #[test]
    fn extracto_una_linea_y_240_caracteres() {
        let texto = format!("  Hola\n\n  mundo\tcon   espacios {}", "á".repeat(300));
        let limpio = extracto(&texto);
        assert!(!limpio.contains('\n'));
        assert!(limpio.starts_with("Hola mundo con espacios"));
        assert_eq!(limpio.chars().count(), 240);
    }

    #[test]
    fn los_ultimos_son_diez_y_del_mas_nuevo_al_mas_viejo() {
        let comentarios: Vec<Comentario> = (0..15)
            .map(|indice| Comentario {
                id: indice.to_string(),
                autor: "Ana".into(),
                mio: false,
                fecha: format!("2026-11-{:02}T10:00:00+01:00", indice + 1),
                extracto: format!("c{indice}"),
            })
            .collect();
        let ultimos = ultimos_comentarios(comentarios);
        assert_eq!(ultimos.len(), 10);
        assert_eq!(ultimos[0].id, "14");
        assert_eq!(ultimos[9].id, "5");
    }

    #[test]
    fn resumen_legible_de_cambios() {
        let cambios = vec![
            Cambio::Comentarios {
                n: 1,
                autores: vec!["Ana".into()],
                extracto: "hola".into(),
            },
            Cambio::Estado {
                antes: Some("En curso".into()),
                despues: Some("LISTO".into()),
            },
            Cambio::Fusionada,
        ];
        assert_eq!(
            resumen_cambios(&cambios),
            "Ana comentó · Estado: En curso → LISTO"
        );
        assert_eq!(
            resumen_cambios(&[Cambio::Comentarios {
                n: 3,
                autores: vec!["Ana".into(), "Luis".into()],
                extracto: "x".into()
            }]),
            "3 comentarios"
        );
    }

    #[test]
    fn selecciona_por_ventana_fijadas_y_sin_huella() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let conn = db::abrir(&ruta).expect("BD");
        for (clave, dia, fijada) in [
            ("jira:PROJ-1", "2026-11-04T09:00:00+01:00", 0),
            ("jira:PROJ-2", "2026-09-01T09:00:00+01:00", 1),
            ("jira:PROJ-3", "2026-09-01T09:00:00+01:00", 0),
        ] {
            conn.execute(
                "INSERT INTO recurso (clave, tipo, url, proyecto, seguir) VALUES (?1, 'jira', ?2, 'PROJ', 1)",
                params![clave, format!("https://x/browse/{clave}")],
            )
            .expect("recurso");
            conn.execute(
                "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo, recurso, fijada)
                 VALUES (substr(?1, 1, 10), ?1, ?1, 'enlace', 'navegador', 'jira', 'x', ?2, ?3)",
                params![dia, clave, fijada],
            )
            .expect("entrada");
        }
        let aj = Ajustes {
            dias_seguimiento: 21,
            ..Ajustes::default()
        };
        let seleccion = seleccionar(&ruta, &aj, false, cuando(4)).expect("selecciona");
        let claves: Vec<&str> = seleccion.iter().map(|fila| fila.clave.as_str()).collect();
        assert_eq!(claves, vec!["jira:PROJ-1", "jira:PROJ-2"]);

        let sin_huella = seleccionar(&ruta, &aj, true, cuando(4)).expect("sin huella");
        assert_eq!(sin_huella.len(), 2);

        conn.execute(
            "UPDATE recurso SET huella_actual = '{}' WHERE clave = 'jira:PROJ-1'",
            [],
        )
        .expect("huella");
        let sin_huella = seleccionar(&ruta, &aj, true, cuando(4)).expect("sin huella 2");
        assert_eq!(sin_huella.len(), 1);
        assert_eq!(sin_huella[0].clave, "jira:PROJ-2");
    }

    #[test]
    fn entidades_de_recurso_incluyen_refs_app_wi_y_proyecto() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = db::abrir(&tmp.path().join("migas.db")).expect("BD");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url, proyecto, seguir)
             VALUES ('issue:equipo/despliegues#1234', 'issue',
                     'https://gitlab/equipo/despliegues/-/issues/1234',
                     'equipo/despliegues', 1)",
            [],
        )
        .expect("recurso");
        conn.execute(
            "INSERT INTO recurso_entidad (clave, valor)
             VALUES ('issue:equipo/despliegues#1234', 'vieja:1')",
            [],
        )
        .expect("entidad vieja");
        let fila = Seguido {
            clave: "issue:equipo/despliegues#1234".into(),
            tipo: "issue".into(),
            proyecto: Some("equipo/despliegues".into()),
        };
        let remoto = Remoto {
            huella: Huella::default(),
            texto: "Desplegar la app. Ver PROJ-12 y FS#45600.".into(),
            titulo: None,
            estado: None,
            app: Some("informes-web".into()),
            identidad_error: None,
        };
        escribir_entidades(&conn, &fila, &remoto, &Ajustes::default()).expect("entidades");
        let valores: Vec<String> = {
            let mut consulta = conn
                .prepare("SELECT valor FROM recurso_entidad WHERE clave = ?1 ORDER BY valor")
                .expect("consulta");
            consulta
                .query_map([&fila.clave], |fila| fila.get(0))
                .expect("filas")
                .collect::<Result<Vec<String>, _>>()
                .expect("valores")
        };
        assert!(valores.contains(&"app:informes-web".to_string()));
        assert!(valores.contains(&"wi:1234".to_string()));
        assert!(
            !valores.contains(&"proyecto:equipo/despliegues".to_string()),
            "el repo de WI no puede ser proyecto"
        );
        assert!(valores.contains(&"jira:PROJ-12".to_string()));
        assert!(valores.contains(&"fs:45600".to_string()));
        assert!(
            !valores.contains(&"vieja:1".to_string()),
            "las viejas se borran"
        );
    }

    #[test]
    fn el_manejador_es_clone_send_sync() {
        fn comprobar<T: Clone + Send + Sync>() {}
        comprobar::<ManejadorSeguimiento>();
    }

    fn preparar(
        ruta: &Path,
        clave: &str,
        tipo: &str,
        fuente: &str,
        url: &str,
        proyecto: Option<&str>,
        ahora: DateTime<Local>,
    ) {
        let conn = db::abrir(ruta).expect("BD");
        conn.execute(
            "INSERT INTO recurso (clave, tipo, url, proyecto, seguir)
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![clave, tipo, url, proyecto],
        )
        .expect("recurso");
        conn.execute(
            "INSERT INTO entrada (dia, creada, actualizada, tipo, origen, fuente, titulo, recurso)
             VALUES (?1, ?2, ?2, 'enlace', 'navegador', ?3, 'x', ?4)",
            params![
                ahora.format("%Y-%m-%d").to_string(),
                ahora.to_rfc3339(),
                fuente,
                clave
            ],
        )
        .expect("entrada");
    }

    fn comentario(ajeno: bool) -> Value {
        let (id, autor, cuenta) = if ajeno {
            ("1", "Ana", "acc-2")
        } else {
            ("2", "Yo", "acc-1")
        };
        serde_json::json!({
            "id": id,
            "author": {"displayName": autor, "accountId": cuenta},
            "created": "2026-11-04T11:00:00.000+0100",
            "body": {"type": "doc", "version": 1, "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Comentario de prueba"}]}
            ]}
        })
    }

    fn montar(servidor: &MockServer, comentarios: Vec<Value>) {
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/myself");
            then.status(200)
                .json_body(serde_json::json!({"accountId": "acc-1"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-12");
            then.status(200).json_body(serde_json::json!({
                "key": "PROJ-12",
                "fields": {
                    "summary": "Listado de notas",
                    "status": {"name": "En curso"},
                    "assignee": {"displayName": "Ana"},
                    "updated": "2026-11-04T09:00:00.000+0100",
                    "description": {"type": "doc", "version": 1, "content": [
                        {"type": "paragraph", "content": [{"type": "text", "text": "Ver PROJ-12"}]}
                    ]}
                }
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-12/comment");
            then.status(200).json_body(serde_json::json!({
                "total": comentarios.len(),
                "comments": comentarios
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(200)
                .json_body(serde_json::json!({"username": "yo"}));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v4/projects/grupo%2Fproy/merge_requests/39");
            then.status(200).json_body(serde_json::json!({
                "title": "MR de descarga",
                "state": "opened",
                "assignees": [{"name": "Ana"}],
                "labels": ["front"],
                "head_pipeline": {"status": "running"},
                "updated_at": "2026-11-04T09:00:00Z",
                "user_notes_count": 0,
                "description": "Ver PROJ-12"
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v4/projects/grupo%2Fproy/merge_requests/39/notes");
            then.status(200).json_body(serde_json::json!([]));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/agents/me");
            then.status(200)
                .json_body(serde_json::json!({"agent": {"id": 7}}));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/5")
                .query_param_missing("include");
            then.status(200).json_body(serde_json::json!({
                "ticket": {
                    "id": 5,
                    "subject": "Ticket de soporte",
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
                .query_param("per_page", "100");
            then.status(200).json_body(serde_json::json!([]));
        });
    }

    fn ajustes_mock(base: &str) -> Ajustes {
        Ajustes {
            jira_base_url: base.to_string(),
            gitlab_base_url: base.to_string(),
            fs_base_url: base.to_string(),
            ..Ajustes::default()
        }
    }

    fn credenciales_todas() -> Credenciales {
        Credenciales {
            jira_user: Some("yo@ejemplo.es".into()),
            jira_token: Some("token-jira".into()),
            gitlab_token: Some("token-gitlab".into()),
            fs_key: Some("clave-fs".into()),
            ia_key: None,
        }
    }

    #[tokio::test]
    async fn el_fallo_de_identidad_se_comparte_en_la_vuelta_y_se_reintenta() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let ahora = cuando(4);
        for iid in 1..=6u64 {
            preparar(
                &ruta,
                &format!("mr:grupo/proy!{iid}"),
                "mr",
                "gitlab",
                &format!("https://gitlab/grupo/proy/-/merge_requests/{iid}"),
                Some("grupo/proy"),
                ahora,
            );
        }
        let servidor = MockServer::start();
        let usuario = servidor.mock(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(401);
        });
        let cred = credenciales_todas();
        let ident = Identidad::default();
        let aj = ajustes_mock(&servidor.base_url());

        let informe = vuelta(&ruta, &cred, &aj, &ident, false, ahora).await;
        assert_eq!(informe.seguidos, 6);
        assert_eq!(
            informe.errores,
            vec!["GitLab respondió 401 al leer tu usuario".to_string()]
        );
        usuario.assert_calls(1);

        // La vuelta siguiente reintenta la identidad una sola vez más.
        let informe = vuelta(&ruta, &cred, &aj, &ident, false, ahora).await;
        assert_eq!(
            informe.errores,
            vec!["GitLab respondió 401 al leer tu usuario".to_string()]
        );
        usuario.assert_calls(2);
    }

    #[tokio::test]
    async fn fs_sigue_el_ticket_aunque_falle_la_identidad() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let ahora = cuando(4);
        preparar(
            &ruta,
            "fs:5",
            "fs",
            "fs",
            "https://fs/a/tickets/5",
            None,
            ahora,
        );
        let servidor = MockServer::start();
        let agente = servidor.mock(|when, then| {
            when.method(GET).path("/api/v2/agents/me");
            then.status(403);
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/5")
                .query_param_missing("include");
            then.status(200).json_body(serde_json::json!({
                "ticket": {
                    "id": 5,
                    "subject": "Ticket a medias",
                    "status": 2,
                    "updated_at": "2026-11-04T09:00:00Z"
                }
            }));
        });
        servidor.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/tickets/5/conversations")
                .query_param("per_page", "100");
            then.status(200).json_body(serde_json::json!([
                {"id": 2, "user_id": 7, "body_text": "La mía",
                 "created_at": "2026-11-04T10:00:00Z", "incoming": false},
                {"id": 1, "user_id": 9, "body_text": "La suya",
                 "created_at": "2026-11-03T10:00:00Z", "incoming": true}
            ]));
        });

        let cred = Credenciales {
            fs_key: Some("clave-de-prueba".into()),
            ..Credenciales::default()
        };
        let informe = vuelta(
            &ruta,
            &cred,
            &ajustes_mock(&servidor.base_url()),
            &Identidad::default(),
            false,
            ahora,
        )
        .await;
        agente.assert_calls(1);
        assert_eq!(informe.seguidos, 1);
        assert_eq!(
            informe.errores,
            vec!["Freshservice respondió 403 al leer tu agente".to_string()]
        );
        assert!(informe.avisos.is_empty());

        let conn = db::abrir(&ruta).expect("BD");
        let (huella, vista, comprobado, error): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT huella_actual, huella_vista, comprobado, error
                   FROM recurso WHERE clave = 'fs:5'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?, fila.get(2)?, fila.get(3)?)),
            )
            .expect("recurso");
        assert!(huella.is_some(), "el ticket se sigue igualmente");
        assert!(vista.is_some(), "la primera huella se copia");
        assert!(comprobado.is_some());
        assert!(
            error.is_none(),
            "el fallo de identidad no es error del recurso"
        );
        let huella: Huella = serde_json::from_str(&huella.expect("huella")).expect("json");
        assert_eq!(huella.comentarios_total, 2);
        assert_eq!(huella.ultimos.len(), 2);
        assert!(
            huella.ultimos.iter().all(|comentario| !comentario.mio),
            "sin identidad, ningún comentario puede ser mío"
        );
    }

    #[tokio::test]
    async fn vuelta_completa_huellas_cambios_avisos_y_visto() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let ahora = cuando(4);
        preparar(
            &ruta,
            "jira:PROJ-12",
            "jira",
            "jira",
            "https://jira/browse/PROJ-12",
            Some("PROJ"),
            ahora,
        );
        preparar(
            &ruta,
            "mr:grupo/proy!39",
            "mr",
            "gitlab",
            "https://gitlab/grupo/proy/-/merge_requests/39",
            Some("grupo/proy"),
            ahora,
        );
        preparar(
            &ruta,
            "fs:5",
            "fs",
            "fs",
            "https://fs/a/tickets/5",
            None,
            ahora,
        );
        let ident = Identidad::default();
        let cred = credenciales_todas();

        let servidor = MockServer::start();
        montar(&servidor, Vec::new());
        let aj = ajustes_mock(&servidor.base_url());
        let informe = vuelta(&ruta, &cred, &aj, &ident, false, ahora).await;
        assert_eq!(informe.seguidos, 3);
        assert!(informe.errores.is_empty(), "{:?}", informe.errores);
        assert!(informe.avisos.is_empty());

        {
            let conn = db::abrir(&ruta).expect("BD");
            for clave in ["jira:PROJ-12", "mr:grupo/proy!39", "fs:5"] {
                let (actual, vista, pendiente, comprobado, error): (
                    Option<String>,
                    Option<String>,
                    i64,
                    Option<String>,
                    Option<String>,
                ) = conn
                    .query_row(
                        "SELECT huella_actual, huella_vista, pendiente_visto, comprobado, error
                           FROM recurso WHERE clave = ?1",
                        [clave],
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
                    .expect("recurso");
                assert!(actual.is_some(), "{clave} sin huella actual");
                assert_eq!(actual, vista, "{clave}: la primera huella se copia");
                assert_eq!(pendiente, 0);
                assert!(comprobado.is_some());
                assert!(error.is_none());
            }
            let valores: Vec<String> = {
                let mut consulta = conn
                    .prepare("SELECT valor FROM recurso_entidad WHERE clave = 'mr:grupo/proy!39' ORDER BY valor")
                    .expect("consulta");
                consulta
                    .query_map([], |fila| fila.get(0))
                    .expect("filas")
                    .collect::<Result<Vec<String>, _>>()
                    .expect("valores")
            };
            assert!(valores.contains(&"jira:PROJ-12".to_string()), "{valores:?}");
            assert!(
                valores.contains(&"proyecto:grupo/proy".to_string()),
                "{valores:?}"
            );
            assert!(valores.contains(&"app:proy".to_string()), "{valores:?}");
            // La entrada se reindexa con el estado del recurso.
            let fts: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM entrada_fts WHERE entrada_fts MATCH '\"opened\"*'",
                    [],
                    |fila| fila.get(0),
                )
                .expect("FTS");
            assert_eq!(fts, 1, "el estado del MR debe estar en el índice");
        }

        // Con huella ya no se selecciona nada en la vuelta rápida.
        let rapida = vuelta(&ruta, &cred, &aj, &ident, true, ahora).await;
        assert_eq!(rapida.seguidos, 0);

        // Segunda vuelta: un comentario ajeno en Jira.
        let servidor = MockServer::start();
        montar(&servidor, vec![comentario(true)]);
        let aj = ajustes_mock(&servidor.base_url());
        let informe = vuelta(&ruta, &cred, &aj, &ident, false, ahora).await;
        assert_eq!(informe.seguidos, 3);
        assert_eq!(informe.avisos.len(), 1);
        assert_eq!(informe.avisos[0].titulo, "migas · PROJ-12");
        assert!(
            informe.avisos[0].cuerpo.contains("Ana comentó"),
            "{:?}",
            informe.avisos[0]
        );
        {
            let conn = db::abrir(&ruta).expect("BD");
            let (cambios, vista): (Option<String>, String) = conn
                .query_row(
                    "SELECT cambios, huella_vista FROM recurso WHERE clave = 'jira:PROJ-12'",
                    [],
                    |fila| Ok((fila.get(0)?, fila.get(1)?)),
                )
                .expect("recurso");
            let cambios: Vec<Cambio> =
                serde_json::from_str(cambios.as_deref().expect("cambios")).expect("json");
            assert!(matches!(
                cambios.first(),
                Some(Cambio::Comentarios { n: 1, .. })
            ));
            let vista: Huella = serde_json::from_str(&vista).expect("huella vista");
            assert_eq!(
                vista.comentarios_total, 0,
                "la vista no avanza sin marcar visto"
            );
            crate::recurso::marcar_visto(&conn, "jira:PROJ-12", ahora).expect("marca visto");
        }

        // Tercera vuelta: ya visto, sin cambios ni aviso.
        let informe = vuelta(&ruta, &cred, &aj, &ident, false, ahora).await;
        assert!(informe.avisos.is_empty());
        let conn = db::abrir(&ruta).expect("BD");
        let (cambios, vista): (Option<String>, String) = conn
            .query_row(
                "SELECT cambios, huella_vista FROM recurso WHERE clave = 'jira:PROJ-12'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("recurso");
        assert!(cambios.is_none());
        let vista: Huella = serde_json::from_str(&vista).expect("huella vista");
        assert_eq!(vista.comentarios_total, 1);
    }

    #[tokio::test]
    async fn comentario_solo_mio_avanza_la_vista_sin_cambios() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let ahora = cuando(4);
        preparar(
            &ruta,
            "jira:PROJ-12",
            "jira",
            "jira",
            "https://jira/browse/PROJ-12",
            Some("PROJ"),
            ahora,
        );
        let ident = Identidad::default();
        let cred = credenciales_todas();

        let servidor = MockServer::start();
        montar(&servidor, Vec::new());
        vuelta(
            &ruta,
            &cred,
            &ajustes_mock(&servidor.base_url()),
            &ident,
            false,
            ahora,
        )
        .await;

        let servidor = MockServer::start();
        montar(&servidor, vec![comentario(false)]);
        let informe = vuelta(
            &ruta,
            &cred,
            &ajustes_mock(&servidor.base_url()),
            &ident,
            false,
            ahora,
        )
        .await;
        assert!(informe.avisos.is_empty());
        let conn = db::abrir(&ruta).expect("BD");
        let (cambios, vista): (Option<String>, String) = conn
            .query_row(
                "SELECT cambios, huella_vista FROM recurso WHERE clave = 'jira:PROJ-12'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("recurso");
        assert!(cambios.is_none());
        let vista: Huella = serde_json::from_str(&vista).expect("huella vista");
        assert_eq!(vista.comentarios_total, 1, "la vista avanza con lo mío");
    }

    #[tokio::test]
    async fn un_404_deja_de_seguir_el_recurso() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let ahora = cuando(4);
        preparar(
            &ruta,
            "jira:PROJ-99",
            "jira",
            "jira",
            "https://jira/browse/PROJ-99",
            Some("PROJ"),
            ahora,
        );
        let servidor = MockServer::start();
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/myself");
            then.status(200)
                .json_body(serde_json::json!({"accountId": "acc-1"}));
        });
        servidor.mock(|when, then| {
            when.method(GET).path("/rest/api/3/issue/PROJ-99");
            then.status(404);
        });
        let informe = vuelta(
            &ruta,
            &credenciales_todas(),
            &ajustes_mock(&servidor.base_url()),
            &Identidad::default(),
            false,
            ahora,
        )
        .await;
        assert_eq!(informe.seguidos, 1);
        assert!(
            informe
                .errores
                .contains(&"No existe o sin acceso".to_string())
        );
        let conn = db::abrir(&ruta).expect("BD");
        let (seguir, error): (i64, Option<String>) = conn
            .query_row(
                "SELECT seguir, error FROM recurso WHERE clave = 'jira:PROJ-99'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("recurso");
        assert_eq!(seguir, 0);
        assert_eq!(error.as_deref(), Some("No existe o sin acceso"));
    }

    #[test]
    fn el_planificador_hace_una_vuelta_y_publica_el_estado() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        let _ = db::abrir(&ruta).expect("BD");
        let estados = Arc::new(Mutex::new(Vec::new()));
        let copia = Arc::clone(&estados);
        let manejador = arrancar(
            ruta,
            Credenciales::default(),
            Arc::new(RwLock::new(Ajustes::default())),
            move |estado| {
                if let Ok(mut lista) = copia.lock() {
                    lista.push(estado.en_curso);
                }
            },
        );
        manejador.comprobar_ahora();
        for _ in 0..100 {
            if manejador.estado().ultima.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let estado = manejador.estado();
        assert!(estado.ultima.is_some());
        assert!(!estado.en_curso);
        assert!(
            !estado
                .errores
                .iter()
                .any(|error| error.contains("credenciales"))
        );
        let publicados = estados.lock().expect("estados").clone();
        assert!(publicados.contains(&true));
        assert_eq!(publicados.last(), Some(&false));
    }

    #[test]
    fn actualiza_titulo_por_defecto_y_reindexa() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let ruta = tmp.path().join("migas.db");
        preparar(
            &ruta,
            "jira:PROJ-12",
            "jira",
            "jira",
            "https://empresa.atlassian.net/browse/PROJ-12",
            Some("PROJ"),
            cuando(4),
        );
        let conn = db::abrir(&ruta).expect("BD");
        conn.execute(
            "UPDATE entrada
                SET titulo = 'empresa.atlassian.net',
                    url = 'https://empresa.atlassian.net/browse/PROJ-12'
              WHERE recurso = 'jira:PROJ-12'",
            [],
        )
        .expect("título por defecto");
        let fila = Seguido {
            clave: "jira:PROJ-12".into(),
            tipo: "jira".into(),
            proyecto: Some("PROJ".into()),
        };
        let remoto = Remoto {
            huella: Huella::default(),
            texto: String::new(),
            titulo: Some("Listado de notas".into()),
            estado: None,
            app: None,
            identidad_error: None,
        };
        actualizar_titulos_por_defecto(&conn, &fila, &remoto).expect("título remoto");
        let (id, titulo): (i64, String) = conn
            .query_row(
                "SELECT id, titulo FROM entrada WHERE recurso = 'jira:PROJ-12'",
                [],
                |fila| Ok((fila.get(0)?, fila.get(1)?)),
            )
            .expect("entrada");
        assert_eq!(titulo, "Listado de notas");
        captura::reindexar(&conn, id).expect("reindexar");
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entrada_fts WHERE entrada_fts MATCH '\"Listado\"*'",
                [],
                |fila| fila.get(0),
            )
            .expect("FTS");
        assert_eq!(fts, 1, "el título remoto debe quedar indexado");
    }
}
