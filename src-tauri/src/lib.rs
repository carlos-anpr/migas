//! migas — punto de entrada y comandos IPC. Ver docs/ARQUITECTURA.md.

pub mod adf;
pub mod captura;
pub mod config;
pub mod consultas;
pub mod db;
pub mod entidades;
pub mod error;
pub mod extraer;
pub mod host;
pub mod http;
pub mod huella;
pub mod ia;
pub mod instalar;
pub mod model;
pub mod notificar;
pub mod recurso;
pub mod relaciones;
pub mod seguimiento;
pub mod seleccion;
pub mod tecla;
pub mod util;
pub mod vigia;

use crate::config::Credenciales;
use crate::model::{
    Ajustes, CapturaRapida, DetalleEntrada, DiaResumen, EstadoApp, EstadoFuente, Filtro, Fuente,
    Integraciones, Listado, Nota, NuevaEntrada, ResultadoCaptura, ResumenDia,
};
use chrono::Local;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

/// Estado gestionado por Tauri.
struct Estado {
    bd: Mutex<rusqlite::Connection>,
    ajustes: Arc<RwLock<Ajustes>>,
    cred: Credenciales,
    ruta_bd: PathBuf,
    seguimiento: OnceLock<seguimiento::ManejadorSeguimiento>,
    rapida: Mutex<CapturaRapida>,
    /// Huellas de la última selección y portapapeles vistos por la rápida.
    vistos: Mutex<seleccion::Vistos>,
    bandeja: Mutex<Option<TrayIcon>>,
    tecla_activo: Arc<AtomicBool>,
    tecla_ms: Arc<AtomicU32>,
    tecla_estado: OnceLock<Arc<Mutex<String>>>,
    /// Id de `--mostrar` que el frontend aún no ha recogido.
    pendiente: Mutex<Option<i64>>,
}

impl Estado {
    fn ajustes(&self) -> Ajustes {
        self.ajustes
            .read()
            .map(|guardia| guardia.clone())
            .unwrap_or_default()
    }

    fn bd(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, String> {
        self.bd
            .lock()
            .map_err(|_| "Estado de la base de datos no disponible".to_string())
    }

    fn contar_cambios(&self) -> Option<u32> {
        let conn = self.bd.lock().ok()?;
        consultas::contar_cambios(&conn).ok()
    }
}

/// Ejecuta trabajo bloqueante fuera del hilo de la interfaz.
async fn bloqueante<T, F>(trabajo: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(trabajo)
        .await
        .map_err(|error| format!("La operación bloqueante no terminó: {error}"))?
}

/// Igual que `bloqueante`, pero el estado se obtiene dentro del hilo de trabajo.
async fn con_estado<T, F>(app: AppHandle, trabajo: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Estado) -> Result<T, String> + Send + 'static,
{
    bloqueante(move || {
        let estado = app.state::<Estado>();
        trabajo(&estado)
    })
    .await
}

// ───────────────────────── Comandos IPC ─────────────────────────

#[tauri::command]
async fn listar_dias(
    app: AppHandle,
    desde: Option<String>,
    hasta: Option<String>,
) -> Result<Vec<DiaResumen>, String> {
    con_estado(app, move |estado| {
        let conn = estado.bd()?;
        consultas::listar_dias(&conn, desde.as_deref(), hasta.as_deref()).map_err(String::from)
    })
    .await
}

#[tauri::command]
async fn listar(app: AppHandle, filtro: Filtro) -> Result<Listado, String> {
    con_estado(app, move |estado| {
        let conn = estado.bd()?;
        consultas::listar(&conn, &filtro, Local::now()).map_err(String::from)
    })
    .await
}

#[tauri::command]
async fn detalle(app: AppHandle, id: i64) -> Result<DetalleEntrada, String> {
    con_estado(app, move |estado| {
        let conn = estado.bd()?;
        consultas::detalle(&conn, id).map_err(String::from)
    })
    .await
}

#[tauri::command]
async fn abrir(app: AppHandle, id: i64) -> Result<(), String> {
    let url = con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        Ok(consultas::detalle(&conn, id)
            .map_err(String::from)?
            .entrada
            .url)
    })
    .await?
    .ok_or_else(|| "La entrada no tiene URL".to_string())?;
    bloqueante(move || {
        tauri_plugin_opener::open_url(&url, None::<&str>)
            .map_err(|error| format!("No se pudo abrir la URL: {error}"))
    })
    .await?;
    Ok(())
}

#[tauri::command]
async fn marcar_visto(app: AppHandle, id: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::marcar_visto_entrada(&conn, id, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn marcar_todo_visto(app: AppHandle) -> Result<u32, String> {
    let marcadas = con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::marcar_todo_visto(&conn, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(marcadas)
}

#[tauri::command]
async fn crear_entrada(app: AppHandle, nueva: NuevaEntrada) -> Result<ResultadoCaptura, String> {
    let resultado = con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        let ajustes = estado.ajustes();
        captura::capturar(&conn, &nueva, &ajustes, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(resultado)
}

#[tauri::command]
async fn editar_titulo(app: AppHandle, id: i64, titulo: String) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        let ajustes = estado.ajustes();
        consultas::editar_titulo(&conn, &ajustes, id, &titulo, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn fijar(app: AppHandle, id: i64, fijada: bool) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::fijar(&conn, id, fijada).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn borrar(app: AppHandle, id: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::borrar(&conn, id, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn restaurar(app: AppHandle, id: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::restaurar(&conn, id).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn borrar_definitivo(app: AppHandle, id: i64, sello: String) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::borrar_definitivo(&conn, id, &sello).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn vaciar_papelera(app: AppHandle) -> Result<usize, String> {
    let borradas = con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::vaciar_papelera(&conn).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(borradas)
}

#[tauri::command]
async fn crear_nota(app: AppHandle, entrada_id: i64, texto: String) -> Result<Nota, String> {
    let nota = con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        let ajustes = estado.ajustes();
        consultas::crear_nota(&conn, &ajustes, entrada_id, &texto, Local::now())
            .map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(nota)
}

#[tauri::command]
async fn editar_nota(app: AppHandle, id: i64, texto: String) -> Result<Nota, String> {
    let nota = con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        let ajustes = estado.ajustes();
        consultas::editar_nota(&conn, &ajustes, id, &texto, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(nota)
}

#[tauri::command]
async fn borrar_nota(app: AppHandle, id: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        let ajustes = estado.ajustes();
        consultas::borrar_nota(&conn, &ajustes, id).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn enlazar(app: AppHandle, a: i64, b: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::enlazar(&conn, a, b, Local::now()).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn desenlazar(app: AppHandle, a: i64, b: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::desenlazar(&conn, a, b).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn descartar(app: AppHandle, a: i64, b: i64) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::descartar(&conn, a, b).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn seguir(app: AppHandle, id: i64, seguir: bool) -> Result<(), String> {
    con_estado(app.clone(), move |estado| {
        let conn = estado.bd()?;
        consultas::seguir(&conn, id, seguir).map_err(String::from)
    })
    .await?;
    emitir_datos(&app);
    Ok(())
}

#[tauri::command]
async fn comprobar_ahora(app: AppHandle) -> Result<(), String> {
    con_estado(app, |estado| {
        let manejador = estado
            .seguimiento
            .get()
            .ok_or_else(|| "El seguimiento no está listo".to_string())?;
        manejador.comprobar_ahora();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn estado_app(app: AppHandle) -> Result<EstadoApp, String> {
    con_estado(app, |estado| {
        let ajustes = estado.ajustes();
        let (cambios, extension) = {
            let conn = estado.bd()?;
            let cambios = consultas::contar_cambios(&conn).map_err(String::from)?;
            let extension = db::meta_leer(&conn, "ultima_extension").map_err(String::from)?;
            (cambios, extension)
        };
        let seguimiento = estado
            .seguimiento
            .get()
            .map(|manejador| manejador.estado())
            .unwrap_or_default();
        let doble_ctrl = estado
            .tecla_estado
            .get()
            .and_then(|estado_tecla| estado_tecla.lock().ok().map(|guardia| guardia.clone()))
            .unwrap_or_else(|| {
                if ajustes.doble_ctrl {
                    "sin teclado".to_string()
                } else {
                    "desactivado".to_string()
                }
            });
        let (host_registrado, autostart) = instalar::estado();
        Ok(EstadoApp {
            version: env!("CARGO_PKG_VERSION").to_string(),
            seguimiento,
            fuentes: vec![
                EstadoFuente {
                    fuente: Fuente::Jira,
                    configurada: estado.cred.jira_user.is_some()
                        && estado.cred.jira_token.is_some(),
                    error: None,
                },
                EstadoFuente {
                    fuente: Fuente::Gitlab,
                    configurada: estado.cred.gitlab_token.is_some(),
                    error: None,
                },
                EstadoFuente {
                    fuente: Fuente::Fs,
                    configurada: estado.cred.fs_key.is_some() && estado.cred.jira_user.is_some(),
                    error: None,
                },
            ],
            integraciones: Integraciones {
                host_registrado,
                extension_vista: extension,
                doble_ctrl,
                autostart,
            },
            cambios,
            ruta_bd: estado.ruta_bd.to_string_lossy().to_string(),
            ia_disponible: ajustes.ia && estado.cred.ia_key.is_some(),
        })
    })
    .await
}

#[tauri::command]
async fn obtener_ajustes(app: AppHandle) -> Result<Ajustes, String> {
    con_estado(app, |estado| Ok(estado.ajustes())).await
}

#[tauri::command]
async fn guardar_ajustes(app: AppHandle, ajustes: Ajustes) -> Result<(), String> {
    con_estado(app, move |estado| {
        config::guardar_ajustes(&ajustes).map_err(String::from)?;
        recurso::fijar_repo_wi(&ajustes.repo_wi);
        if let Ok(mut guardia) = estado.ajustes.write() {
            *guardia = ajustes.clone();
        }
        estado
            .tecla_activo
            .store(ajustes.doble_ctrl, Ordering::SeqCst);
        estado
            .tecla_ms
            .store(ajustes.doble_ctrl_ms, Ordering::SeqCst);
        if !ajustes.doble_ctrl
            && let Some(estado_tecla) = estado.tecla_estado.get()
            && let Ok(mut guardia) = estado_tecla.lock()
        {
            *guardia = "desactivado".to_string();
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn instalar_integraciones() -> Result<String, String> {
    bloqueante(|| Ok(instalar::instalar().join("\n"))).await
}

#[tauri::command]
async fn ruta_extension() -> Result<String, String> {
    bloqueante(|| {
        let ruta = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("extension");
        std::fs::canonicalize(&ruta)
            .map(|ruta| ruta.to_string_lossy().to_string())
            .map_err(|error| format!("No se encontró la carpeta de la extensión: {error}"))
    })
    .await
}

#[tauri::command]
async fn copiar(texto: String) -> Result<(), String> {
    let copiado = bloqueante(move || {
        Ok(copiar_con("wl-copy", &[], &texto)
            || copiar_con("xclip", &["-selection", "clipboard"], &texto))
    })
    .await?;
    if copiado {
        Ok(())
    } else {
        Err("No se pudo copiar al portapapeles".to_string())
    }
}

#[tauri::command]
async fn captura_pendiente(app: AppHandle) -> Result<CapturaRapida, String> {
    con_estado(app, |estado| {
        estado
            .rapida
            .lock()
            .map(|guardia| guardia.clone())
            .map_err(|_| "Estado de captura no disponible".to_string())
    })
    .await
}

#[tauri::command]
async fn abrir_rapida(app: AppHandle) -> Result<(), String> {
    con_estado(app.clone(), |estado| {
        let mut rapida = estado
            .rapida
            .lock()
            .map_err(|_| "Estado de captura no disponible".to_string())?;
        *rapida = CapturaRapida::default();
        Ok(())
    })
    .await?;
    mostrar_rapida(&app);
    Ok(())
}

fn ocultar_principal_ventana(app: &AppHandle) -> Result<(), String> {
    let Some(ventana) = app.get_webview_window("main") else {
        return Err("No se encontró la ventana principal".to_string());
    };
    ventana
        .hide()
        .map_err(|error| format!("No se pudo ocultar la ventana: {error}"))
}

#[tauri::command]
fn ocultar_principal(app: AppHandle) -> Result<(), String> {
    ocultar_principal_ventana(&app)
}

#[tauri::command]
fn cerrar_rapida(app: AppHandle) -> Result<(), String> {
    if let Some(ventana) = app.get_webview_window("rapida") {
        let _ = ventana.hide();
    }
    Ok(())
}

#[tauri::command]
fn mostrar_principal(app: AppHandle, id: Option<i64>) -> Result<(), String> {
    mostrar_principal_ventana(&app, id, false);
    Ok(())
}

#[tauri::command]
async fn mostrar_pendiente(app: AppHandle) -> Result<Option<i64>, String> {
    con_estado(app, |estado| {
        estado
            .pendiente
            .lock()
            .map(|mut guardia| guardia.take())
            .map_err(|_| "Estado de selección pendiente no disponible".to_string())
    })
    .await
}

#[tauri::command]
async fn resumen_dia(app: AppHandle, dia: String) -> Result<Option<ResumenDia>, String> {
    con_estado(app, move |estado| {
        let conn = estado.bd()?;
        ia::leer_resumen(&conn, &dia).map_err(String::from)
    })
    .await
}

#[tauri::command]
async fn generar_resumen(app: AppHandle, dia: String) -> Result<ResumenDia, String> {
    let (ruta, cred, aj) = con_estado(app, |estado| {
        Ok((
            estado.ruta_bd.clone(),
            estado.cred.clone(),
            estado.ajustes(),
        ))
    })
    .await?;
    if !aj.ia
        || cred
            .ia_key
            .as_deref()
            .filter(|clave| !clave.is_empty())
            .is_none()
    {
        return Err("Activa la IA en Ajustes".to_string());
    }
    ia::resumir_dia(&ruta, &cred, &aj, &dia)
        .await
        .map_err(String::from)
}

#[tauri::command]
fn salir(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

// ───────────────────────── Ayudas de ventanas y eventos ─────────────────────────

fn emitir_datos(app: &AppHandle) {
    let _ = app.emit("migas://datos", ());
}

fn mostrar_principal_ventana(app: &AppHandle, id: Option<i64>, _primera: bool) {
    if let Some(ventana) = app.get_webview_window("main") {
        let _ = ventana.unminimize();
        let _ = ventana.show();
        let _ = ventana.set_focus();
    }
    if let Some(id) = id {
        if let Ok(mut pendiente) = app.state::<Estado>().pendiente.lock() {
            *pendiente = Some(id);
        }
        // El frontend puede haber arrancado ya (segunda instancia) o recuperarlo con
        // `mostrar_pendiente` si este evento llega antes de que escuche.
        let _ = app.emit("migas://mostrar", json!({ "id": id }));
    }
}

fn rapida_visible(app: &AppHandle) -> bool {
    app.get_webview_window("rapida")
        .and_then(|ventana| ventana.is_visible().ok())
        .unwrap_or(false)
}

fn mostrar_rapida(app: &AppHandle) {
    let Some(ventana) = app.get_webview_window("rapida") else {
        return;
    };
    posicionar_rapida(&ventana);
    let _ = ventana.show();
    let _ = ventana.set_focus();
    let captura = app
        .state::<Estado>()
        .rapida
        .lock()
        .map(|guardia| guardia.clone())
        .unwrap_or_default();
    let _ = app.emit("migas://rapida", captura);
}

fn posicionar_rapida(ventana: &tauri::WebviewWindow) {
    let monitor = ventana
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| ventana.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let Ok(tamano) = ventana.outer_size() else {
        return;
    };
    let posicion = monitor.position();
    let area = monitor.size();
    let x = posicion.x + (area.width as i32 - tamano.width as i32) / 2;
    let y = posicion.y + (area.height as i32 * 18 / 100);
    let _ = ventana.set_position(tauri::PhysicalPosition::new(x, y));
}

fn abrir_rapida_con_seleccion(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let primaria = seleccion::primaria();
        let portapapeles = seleccion::portapapeles();
        let (primaria, portapapeles) = if let Some(estado) = app.try_state::<Estado>() {
            let mut vistos = match estado.vistos.lock() {
                Ok(guardia) => guardia,
                Err(veneno) => veneno.into_inner(),
            };
            seleccion::solo_nuevos(primaria, portapapeles, &mut vistos)
        } else {
            (primaria, portapapeles)
        };
        if let Some(estado) = app.try_state::<Estado>()
            && let Ok(mut rapida) = estado.rapida.lock()
        {
            *rapida = CapturaRapida {
                primaria,
                portapapeles,
            };
        }
        mostrar_rapida(&app);
    });
}

fn entrada_reciente_de_navegador(app: &AppHandle) -> bool {
    let estado = app.state::<Estado>();
    let Ok(conn) = estado.bd.lock() else {
        return false;
    };
    let desde = (Local::now() - chrono::Duration::milliseconds(1500)).to_rfc3339();
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entrada
                        WHERE origen = 'navegador' AND julianday(actualizada) >= julianday(?1))",
        [desde],
        |fila| fila.get::<_, i64>(0),
    )
    .map(|existe| existe != 0)
    .unwrap_or(false)
}

/// Crea la bandeja. El `TrayIcon` devuelto debe conservarse (se guarda en el estado).
fn es_pruebas() -> bool {
    std::env::var_os("MIGAS_DATA_DIR").is_some()
}

fn tooltip_bandeja(cambios: Option<u32>) -> String {
    if es_pruebas() {
        "migas (pruebas)".to_string()
    } else if let Some(cambios) = cambios {
        format!("migas · {cambios} con cambios")
    } else {
        "migas".to_string()
    }
}

/// Crea la bandeja. El `TrayIcon` devuelto debe conservarse (se guarda en el estado).
fn crear_bandeja(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let abrir = MenuItem::with_id(app, "abrir", "Abrir migas", true, None::<&str>)?;
    let capturar = MenuItem::with_id(app, "capturar", "Guardar selección", true, None::<&str>)?;
    let comprobar = MenuItem::with_id(app, "comprobar", "Comprobar cambios", true, None::<&str>)?;
    let separador = PredefinedMenuItem::separator(app)?;
    let salir = MenuItem::with_id(app, "salir", "Salir", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&abrir, &capturar, &comprobar, &separador, &salir])?;
    let icono = tauri::image::Image::from_bytes(include_bytes!("../icons/bandeja.png"))?;

    TrayIconBuilder::with_id("migas")
        .icon(icono)
        .tooltip(tooltip_bandeja(None))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, evento| match evento.id().as_ref() {
            "abrir" => mostrar_principal_ventana(app, None, false),
            "capturar" => abrir_rapida_con_seleccion(app),
            "comprobar" => {
                if let Some(manejador) = app.state::<Estado>().seguimiento.get() {
                    manejador.comprobar_ahora();
                }
            }
            "salir" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|bandeja, evento| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = evento
            {
                mostrar_principal_ventana(bandeja.app_handle(), None, false);
            }
        })
        .build(app)
}

fn copiar_con(programa: &str, argumentos: &[&str], texto: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Ok(mut hijo) = Command::new(programa)
        .args(argumentos)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let inicio = Instant::now();
    let limite = inicio + Duration::from_secs(2);
    let error_escritura = Arc::new(AtomicBool::new(false));
    let error_hilo = Arc::clone(&error_escritura);
    let texto = texto.to_string();
    let stdin = hijo.stdin.take();
    let escritor = std::thread::Builder::new()
        .name("migas-copiar".into())
        .spawn(move || {
            if let Some(mut entrada) = stdin {
                if entrada.write_all(texto.as_bytes()).is_err() {
                    error_hilo.store(true, Ordering::SeqCst);
                }
                let _ = entrada.flush();
            }
        });
    if escritor.is_err() {
        let _ = hijo.kill();
        let _ = hijo.wait();
        return false;
    }
    loop {
        match hijo.try_wait() {
            Ok(Some(estado)) => {
                return estado.success() && !error_escritura.load(Ordering::SeqCst);
            }
            Ok(None) => {
                if Instant::now() >= limite {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                let _ = hijo.kill();
                let _ = hijo.wait();
                return false;
            }
        }
    }
}

// ───────────────────────── Argumentos y arranque ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Accion {
    Nada,
    Capturar,
    Mostrar(Option<i64>),
}

fn accion_de_args(args: &[String]) -> Accion {
    let mut capturar = false;
    let mut oculto = false;
    let mut mostrar = None;
    let mut indice = 0;
    while indice < args.len() {
        match args[indice].as_str() {
            "--capturar" => capturar = true,
            "--oculto" => oculto = true,
            "--mostrar" => {
                if let Some(valor) = args.get(indice + 1) {
                    if let Ok(id) = valor.parse::<i64>() {
                        mostrar = Some(id);
                    }
                    indice += 1;
                }
            }
            _ => {}
        }
        indice += 1;
    }
    if let Some(id) = mostrar {
        Accion::Mostrar(Some(id))
    } else if capturar {
        Accion::Capturar
    } else if oculto {
        Accion::Nada
    } else {
        Accion::Mostrar(None)
    }
}

fn atender_args(app: &AppHandle, args: &[String], primera: bool) {
    match accion_de_args(args) {
        Accion::Nada => {}
        Accion::Capturar => abrir_rapida_con_seleccion(app),
        Accion::Mostrar(id) => mostrar_principal_ventana(app, id, primera),
    }
}

fn imprimir_y_salir(lineas: &[String]) -> ! {
    for linea in lineas {
        println!("{linea}");
    }
    let fallo = lineas.iter().any(|linea| linea.starts_with('✗'));
    std::process::exit(if fallo { 1 } else { 0 });
}

/// Punto de entrada del binario: decide el modo (app, host nativo, CLI). Ver §Modos.
pub fn principal() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(argumento) = args.get(1) {
        if argumento.starts_with("chrome-extension://") {
            std::process::exit(host::ejecutar());
        }
        match argumento.as_str() {
            "instalar" => imprimir_y_salir(&instalar::instalar()),
            "desinstalar" => imprimir_y_salir(&instalar::desinstalar()),
            _ => {}
        }
    }

    let ruta_bd = config::dir_datos().join("migas.db");
    let conn = match db::abrir(&ruta_bd) {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!("migas: no se pudo abrir la base de datos: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = db::purgar_papelera(&conn, Local::now()) {
        eprintln!("migas: no se pudo purgar la papelera: {error}");
    }
    let ajustes = config::cargar_ajustes();
    recurso::fijar_repo_wi(&ajustes.repo_wi);
    if let Err(error) = entidades::limpiar_repo_despliegue(&conn, &ajustes) {
        eprintln!("migas: no se pudieron limpiar las entidades del repo de work items: {error}");
    }
    let cred = Credenciales::cargar();
    let estado = Estado {
        bd: Mutex::new(conn),
        ajustes: Arc::new(RwLock::new(ajustes.clone())),
        cred,
        ruta_bd,
        seguimiento: OnceLock::new(),
        rapida: Mutex::new(CapturaRapida::default()),
        vistos: Mutex::new(seleccion::Vistos::default()),
        bandeja: Mutex::new(None),
        tecla_activo: Arc::new(AtomicBool::new(ajustes.doble_ctrl)),
        tecla_ms: Arc::new(AtomicU32::new(ajustes.doble_ctrl_ms)),
        tecla_estado: OnceLock::new(),
        pendiente: Mutex::new(None),
    };

    let mut constructor = tauri::Builder::default();
    if !es_pruebas() {
        constructor = constructor.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            atender_args(app, &argv, false);
        }));
    }
    constructor
        .plugin(tauri_plugin_opener::init())
        .manage(estado)
        .setup(|app| {
            let handle = app.handle().clone();

            if let Some(main) = app.get_webview_window("main") {
                let ventana = main.clone();
                main.on_window_event(move |evento| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = evento {
                        api.prevent_close();
                        let _ = ventana.hide();
                    }
                });
            }

            let estado = app.state::<Estado>();
            let vistos = seleccion::vistos_iniciales();
            if let Ok(mut guardia) = estado.vistos.lock() {
                *guardia = vistos;
            }
            match crear_bandeja(&handle) {
                Ok(bandeja) => {
                    if let Ok(mut guardia) = estado.bandeja.lock() {
                        *guardia = Some(bandeja);
                    }
                }
                Err(error) => eprintln!("migas: no se pudo crear la bandeja: {error}"),
            }

            let app_estado = handle.clone();
            let manejador = seguimiento::Planificador::arrancar(
                estado.ruta_bd.clone(),
                estado.cred.clone(),
                Arc::clone(&estado.ajustes),
                move |estado_seg| {
                    let _ = app_estado.emit("migas://seguimiento", &estado_seg);
                    if let Some(bandeja) = app_estado.tray_by_id("migas") {
                        let cambios = app_estado.state::<Estado>().contar_cambios().unwrap_or(0);
                        let _ = bandeja.set_tooltip(Some(tooltip_bandeja(Some(cambios))));
                    }
                },
            );
            let _ = estado.seguimiento.set(manejador);

            let app_vigia = handle.clone();
            vigia::arrancar(estado.ruta_bd.clone(), move || {
                let _ = app_vigia.emit("migas://datos", ());
                if let Some(manejador) = app_vigia.state::<Estado>().seguimiento.get() {
                    manejador.pendientes();
                }
            });

            let ruta_vocabulario = estado.ruta_bd.clone();
            let ajustes_vocabulario = Arc::clone(&estado.ajustes);
            std::thread::Builder::new()
                .name("migas-vocabulario".into())
                .spawn(move || {
                    let conn = match db::abrir(&ruta_vocabulario) {
                        Ok(conn) => conn,
                        Err(error) => {
                            eprintln!("migas vocabulario: no se pudo abrir la BD: {error}");
                            return;
                        }
                    };
                    loop {
                        let ajustes = ajustes_vocabulario
                            .read()
                            .map(|guardia| guardia.clone())
                            .unwrap_or_default();
                        if let Err(error) = entidades::refrescar_vocabulario(&conn, &ajustes) {
                            eprintln!("migas vocabulario: {error}");
                        }
                        std::thread::sleep(Duration::from_secs(3600));
                    }
                })
                .ok();

            let app_tecla = handle.clone();
            let al_detectar = move || {
                let app = app_tecla.clone();
                std::thread::spawn(move || {
                    if rapida_visible(&app) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(450));
                    if rapida_visible(&app) {
                        return;
                    }
                    if entrada_reciente_de_navegador(&app) {
                        return;
                    }
                    abrir_rapida_con_seleccion(&app);
                });
            };
            let estado_tecla = tecla::arrancar(
                Arc::clone(&estado.tecla_ms),
                Arc::clone(&estado.tecla_activo),
                al_detectar,
            );
            let _ = estado.tecla_estado.set(estado_tecla);

            atender_args(&handle, &std::env::args().collect::<Vec<_>>(), true);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            listar_dias,
            listar,
            detalle,
            abrir,
            marcar_visto,
            marcar_todo_visto,
            crear_entrada,
            editar_titulo,
            fijar,
            borrar,
            restaurar,
            borrar_definitivo,
            vaciar_papelera,
            crear_nota,
            editar_nota,
            borrar_nota,
            enlazar,
            desenlazar,
            descartar,
            seguir,
            comprobar_ahora,
            estado_app,
            obtener_ajustes,
            guardar_ajustes,
            instalar_integraciones,
            ruta_extension,
            copiar,
            captura_pendiente,
            abrir_rapida,
            ocultar_principal,
            cerrar_rapida,
            mostrar_principal,
            mostrar_pendiente,
            resumen_dia,
            generar_resumen,
            salir
        ])
        .run(tauri::generate_context!())
        .expect("No se pudo iniciar migas");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(valores: &[&str]) -> Vec<String> {
        valores.iter().map(|valor| valor.to_string()).collect()
    }

    #[test]
    fn acciones_de_los_argumentos() {
        assert_eq!(
            accion_de_args(&args(&["/bin/migas"])),
            Accion::Mostrar(None)
        );
        assert_eq!(
            accion_de_args(&args(&["/bin/migas", "--oculto"])),
            Accion::Nada
        );
        assert_eq!(
            accion_de_args(&args(&["/bin/migas", "--capturar"])),
            Accion::Capturar
        );
        assert_eq!(
            accion_de_args(&args(&["/bin/migas", "--mostrar", "12"])),
            Accion::Mostrar(Some(12))
        );
        assert_eq!(
            accion_de_args(&args(&["/bin/migas", "--mostrar", "no"])),
            Accion::Mostrar(None)
        );
    }

    #[test]
    fn copiar_con_termina_con_plazo() {
        assert!(copiar_con("cat", &[], "hola"));
        let inicio = std::time::Instant::now();
        assert!(!copiar_con("sh", &["-c", "sleep 3"], "hola"));
        assert!(
            inicio.elapsed() < std::time::Duration::from_secs(3),
            "copiar_con no respetó el plazo"
        );
    }
}
