//! Detección de la doble pulsación de Ctrl con `evdev` (solo la tecla Ctrl; nada más se lee
//! ni se guarda) y detector puro para las pruebas.

use evdev::{Device, EventType, InputEvent, KeyCode, RelativeAxisCode};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Eventos que entiende el detector puro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entrada {
    CtrlAbajo,
    CtrlArriba,
    OtraTecla,
    Rueda,
}

/// Detector puro de la doble pulsación. No toca dispositivos.
#[derive(Debug, Clone)]
pub struct Detector {
    max_ms: u32,
    abajo: Option<Instant>,
    ultima_suelta: Option<Instant>,
    candidato: bool,
    /// Si hay una tercera pulsación en curso (triple), hasta cuándo se ignora.
    bloqueado: Option<Instant>,
    segunda: bool,
    cancelado: bool,
}

impl Detector {
    pub fn new(max_ms: u32) -> Self {
        Self {
            max_ms,
            abajo: None,
            ultima_suelta: None,
            candidato: false,
            bloqueado: None,
            segunda: false,
            cancelado: false,
        }
    }

    pub fn set_max_ms(&mut self, max_ms: u32) {
        self.max_ms = max_ms;
    }

    /// Procesa un evento. Devuelve `true` al soltar la segunda pulsación válida.
    ///
    /// La doble pulsación se confirma en `CtrlArriba`: así se valida que el segundo tap
    /// dura menos de 250 ms y que ninguna otra tecla, botón o rueda se ha colado en el gesto.
    pub fn evento(&mut self, entrada: Entrada, t: Instant) -> bool {
        match entrada {
            Entrada::CtrlAbajo => {
                // Autorepeat: ya está abajo.
                if self.abajo.is_some() {
                    return false;
                }
                if let Some(hasta) = self.bloqueado {
                    if t <= hasta {
                        // Tercera pulsación de un triple: se ignora entera.
                        self.abajo = Some(t);
                        self.segunda = false;
                        self.cancelado = true;
                        self.candidato = false;
                        self.ultima_suelta = None;
                        return false;
                    }
                    self.bloqueado = None;
                }
                let segunda = self.candidato
                    && self.ultima_suelta.is_some_and(|suelta| {
                        t.saturating_duration_since(suelta)
                            < Duration::from_millis(u64::from(self.max_ms))
                    });
                self.abajo = Some(t);
                self.segunda = segunda;
                self.cancelado = false;
                self.candidato = false;
                self.ultima_suelta = None;
                false
            }
            Entrada::CtrlArriba => {
                let Some(abajo) = self.abajo.take() else {
                    self.candidato = false;
                    self.ultima_suelta = None;
                    self.segunda = false;
                    self.bloqueado = None;
                    return false;
                };
                let duracion = t.saturating_duration_since(abajo);
                if self.bloqueado.take().is_some() {
                    self.segunda = false;
                    self.candidato = false;
                    self.ultima_suelta = None;
                    self.cancelado = false;
                    return false;
                }
                if self.segunda {
                    self.segunda = false;
                    let detecta = !self.cancelado && duracion < Duration::from_millis(250);
                    self.cancelado = false;
                    self.candidato = false;
                    self.ultima_suelta = None;
                    if detecta {
                        // La tercera pulsación de un triple no vuelve a disparar.
                        self.bloqueado = Some(t + Duration::from_millis(u64::from(self.max_ms)));
                    }
                    return detecta;
                }
                self.segunda = false;
                if !self.cancelado && duracion < Duration::from_millis(250) {
                    self.ultima_suelta = Some(t);
                    self.candidato = true;
                } else {
                    self.candidato = false;
                    self.ultima_suelta = None;
                }
                self.cancelado = false;
                false
            }
            Entrada::OtraTecla | Entrada::Rueda => {
                if self.abajo.is_some() {
                    self.cancelado = true;
                }
                self.candidato = false;
                self.ultima_suelta = None;
                false
            }
        }
    }
}

/// Estado inicial antes del primer escaneo: si la opción está encendida, el vigilante
/// está activo; no debe aparecer «desactivado» durante los primeros segundos.
fn estado_inicial(activo: bool) -> &'static str {
    if activo { "activo" } else { "desactivado" }
}

/// Arranca el vigilante de teclados. Devuelve el estado legible (`activo`, `desactivado`,
/// `sin permiso`, `sin teclado`) que se actualiza cada 5 s.
pub fn arrancar(
    ms: Arc<AtomicU32>,
    activo: Arc<AtomicBool>,
    al_detectar: impl Fn() + Send + Sync + 'static,
) -> Arc<Mutex<String>> {
    let estado = Arc::new(Mutex::new(
        estado_inicial(activo.load(Ordering::SeqCst)).to_string(),
    ));
    let estado_hilo = Arc::clone(&estado);
    let detector = Arc::new(Mutex::new(Detector::new(ms.load(Ordering::SeqCst))));
    let al_detectar: Arc<dyn Fn() + Send + Sync> = Arc::new(al_detectar);
    let abiertos: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

    let hilo = std::thread::Builder::new()
        .name("migas-tecla".into())
        .spawn(move || {
            loop {
                if !activo.load(Ordering::SeqCst) {
                    fijar_estado(&estado_hilo, "desactivado");
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }

                let mut encontrados = 0usize;
                let mut sin_permiso = false;
                let mut conocidos = match abiertos.lock() {
                    Ok(guardia) => guardia,
                    Err(veneno) => veneno.into_inner(),
                };
                if let Ok(entradas) = std::fs::read_dir("/dev/input") {
                    for entrada in entradas.flatten() {
                        let nombre = entrada.file_name();
                        if !nombre.to_string_lossy().starts_with("event") {
                            continue;
                        }
                        let ruta = entrada.path();
                        if conocidos.contains(&ruta) {
                            encontrados += 1;
                            continue;
                        }
                        match Device::open(&ruta) {
                            Ok(dispositivo) => {
                                let soporta = dispositivo.supported_keys().is_some_and(|teclas| {
                                    teclas.contains(KeyCode::KEY_LEFTCTRL)
                                        || teclas.contains(KeyCode::BTN_LEFT)
                                });
                                if !soporta {
                                    continue;
                                }
                                encontrados += 1;
                                conocidos.insert(ruta.clone());
                                let detector = Arc::clone(&detector);
                                let ms = Arc::clone(&ms);
                                let activo = Arc::clone(&activo);
                                let al_detectar = Arc::clone(&al_detectar);
                                let abiertos_hilo = Arc::clone(&abiertos);
                                let ruta_hilo = ruta.clone();
                                let creado = std::thread::Builder::new()
                                    .name("migas-tecla-dev".into())
                                    .spawn(move || {
                                        leer_dispositivo(
                                            dispositivo,
                                            detector,
                                            ms,
                                            activo,
                                            al_detectar,
                                        );
                                        if let Ok(mut set) = abiertos_hilo.lock() {
                                            set.remove(&ruta_hilo);
                                        }
                                    });
                                if creado.is_err()
                                    && let Ok(mut set) = abiertos.lock()
                                {
                                    set.remove(&ruta);
                                }
                            }
                            Err(error) => {
                                if error.kind() == std::io::ErrorKind::PermissionDenied {
                                    sin_permiso = true;
                                    encontrados += 1;
                                }
                            }
                        }
                    }
                }
                drop(conocidos);
                if let Ok(mut set) = abiertos.lock() {
                    set.retain(|ruta| ruta.exists());
                }
                let abiertos_ahora = abiertos.lock().map(|set| set.len()).unwrap_or(0);
                fijar_estado(
                    &estado_hilo,
                    texto_estado(encontrados, abiertos_ahora, sin_permiso),
                );
                std::thread::sleep(Duration::from_secs(5));
            }
        });
    if let Err(error) = hilo {
        eprintln!("migas tecla: no se pudo arrancar el vigilante: {error}");
    }
    estado
}

fn fijar_estado(estado: &Arc<Mutex<String>>, texto: &str) {
    match estado.lock() {
        Ok(mut guardia) => *guardia = texto.to_string(),
        Err(veneno) => *veneno.into_inner() = texto.to_string(),
    }
}

/// Decide el estado legible. Si no se abrió ningún dispositivo por permiso, ese estado
/// tiene prioridad sobre «sin teclado», aunque no se haya podido contar ninguno.
fn texto_estado(_encontrados: usize, abiertos_ahora: usize, sin_permiso: bool) -> &'static str {
    if abiertos_ahora > 0 {
        "activo"
    } else if sin_permiso {
        "sin permiso"
    } else {
        "sin teclado"
    }
}

fn leer_dispositivo(
    mut dispositivo: Device,
    detector: Arc<Mutex<Detector>>,
    ms: Arc<AtomicU32>,
    activo: Arc<AtomicBool>,
    al_detectar: Arc<dyn Fn() + Send + Sync>,
) {
    loop {
        if !activo.load(Ordering::SeqCst) {
            return;
        }
        let Ok(eventos) = dispositivo.fetch_events() else {
            return;
        };
        let mut disparar = false;
        let Ok(mut detector) = detector.lock() else {
            return;
        };
        detector.set_max_ms(ms.load(Ordering::SeqCst));
        for evento in eventos {
            if !activo.load(Ordering::SeqCst) {
                return;
            }
            if let Some(entrada) = traducir(&evento)
                && detector.evento(entrada, Instant::now())
            {
                disparar = true;
            }
        }
        drop(detector);
        if disparar {
            al_detectar();
        }
    }
}

/// Traduce un evento de `evdev` a lo que entiende el detector. El resto se ignora.
fn traducir(evento: &InputEvent) -> Option<Entrada> {
    let tipo = evento.event_type();
    let codigo = evento.code();
    let valor = evento.value();
    if tipo == EventType::KEY {
        if codigo == KeyCode::KEY_LEFTCTRL.0 {
            match valor {
                1 => Some(Entrada::CtrlAbajo),
                0 => Some(Entrada::CtrlArriba),
                _ => None,
            }
        } else {
            // Cualquier otra tecla o botón (incluido el clic izquierdo) anula la secuencia.
            Some(Entrada::OtraTecla)
        }
    } else if tipo == EventType::RELATIVE {
        let rueda = codigo == RelativeAxisCode::REL_WHEEL.0
            || codigo == RelativeAxisCode::REL_HWHEEL.0
            || codigo == RelativeAxisCode::REL_WHEEL_HI_RES.0
            || codigo == RelativeAxisCode::REL_HWHEEL_HI_RES.0;
        (rueda && valor != 0).then_some(Entrada::Rueda)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instante(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn pulsacion(detector: &mut Detector, base: Instant, inicio: u64, duracion: u64) -> bool {
        detector.evento(Entrada::CtrlAbajo, instante(base, inicio));
        detector.evento(Entrada::CtrlArriba, instante(base, inicio + duracion))
    }

    #[test]
    fn doble_pulsacion_basica() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 80));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 200)));
        assert!(detector.evento(Entrada::CtrlArriba, instante(base, 250)));
    }

    #[test]
    fn tap_lento_no_cuenta() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 300));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 400)));
        assert!(!detector.evento(Entrada::CtrlArriba, instante(base, 450)));
    }

    #[test]
    fn segunda_pulsacion_tarde_no_cuenta() {
        let base = Instant::now();
        let mut detector = Detector::new(200);
        assert!(!pulsacion(&mut detector, base, 0, 50));
        // 200 ms justos después de la suelta: no es menor que el máximo.
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 250)));
        assert!(!detector.evento(Entrada::CtrlArriba, instante(base, 300)));
    }

    #[test]
    fn la_segunda_pulsacion_tiene_ms_configurable() {
        let base = Instant::now();
        let mut corto = Detector::new(100);
        assert!(!pulsacion(&mut corto, base, 0, 50));
        assert!(!corto.evento(Entrada::CtrlAbajo, instante(base, 200)));
        assert!(!corto.evento(Entrada::CtrlArriba, instante(base, 250)));

        let mut largo = Detector::new(500);
        assert!(!pulsacion(&mut largo, base, 0, 50));
        assert!(!largo.evento(Entrada::CtrlAbajo, instante(base, 200)));
        assert!(largo.evento(Entrada::CtrlArriba, instante(base, 250)));
    }

    #[test]
    fn ctrl_c_y_ctrl_clic_anulan() {
        let base = Instant::now();

        let mut con_tecla = Detector::new(330);
        con_tecla.evento(Entrada::CtrlAbajo, instante(base, 0));
        con_tecla.evento(Entrada::OtraTecla, instante(base, 40));
        con_tecla.evento(Entrada::CtrlArriba, instante(base, 80));
        assert!(!con_tecla.evento(Entrada::CtrlAbajo, instante(base, 200)));

        let mut con_clic = Detector::new(330);
        con_clic.evento(Entrada::CtrlAbajo, instante(base, 0));
        con_clic.evento(Entrada::OtraTecla, instante(base, 40));
        con_clic.evento(Entrada::CtrlArriba, instante(base, 80));
        assert!(!con_clic.evento(Entrada::CtrlAbajo, instante(base, 200)));
    }

    #[test]
    fn rueda_entre_pulsaciones_anula() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 50));
        detector.evento(Entrada::Rueda, instante(base, 100));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 150)));
    }

    #[test]
    fn la_tercera_pulsacion_no_vuelve_a_disparar() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 50));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 150)));
        assert!(detector.evento(Entrada::CtrlArriba, instante(base, 200)));
        // Tercera pulsación: no dispara.
        assert!(!pulsacion(&mut detector, base, 250, 50));
    }

    #[test]
    fn ctrl_c_en_la_segunda_pulsacion_no_dispara() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 50));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 150)));
        detector.evento(Entrada::OtraTecla, instante(base, 170));
        assert!(!detector.evento(Entrada::CtrlArriba, instante(base, 200)));
    }

    #[test]
    fn segunda_pulsacion_larga_no_dispara() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 50));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 150)));
        assert!(!detector.evento(Entrada::CtrlArriba, instante(base, 800)));
    }

    #[test]
    fn el_autorepeat_de_ctrl_se_ignora() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        detector.evento(Entrada::CtrlAbajo, instante(base, 0));
        // Repetición mientras sigue abajo: no cancela ni arranca otro tap.
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 30)));
        assert!(!detector.evento(Entrada::CtrlArriba, instante(base, 60)));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 150)));
        assert!(detector.evento(Entrada::CtrlArriba, instante(base, 200)));
    }

    #[test]
    fn una_tecla_entre_pulsaciones_anula() {
        let base = Instant::now();
        let mut detector = Detector::new(330);
        assert!(!pulsacion(&mut detector, base, 0, 50));
        detector.evento(Entrada::OtraTecla, instante(base, 100));
        assert!(!detector.evento(Entrada::CtrlAbajo, instante(base, 150)));
    }

    #[test]
    fn sin_permiso_gana_a_sin_teclado() {
        assert_eq!(texto_estado(0, 0, true), "sin permiso");
        assert_eq!(texto_estado(0, 0, false), "sin teclado");
        assert_eq!(texto_estado(3, 0, true), "sin permiso");
        assert_eq!(texto_estado(3, 0, false), "sin teclado");
        assert_eq!(texto_estado(0, 1, true), "activo");
    }

    #[test]
    fn estado_inicial_respeta_la_opcion() {
        assert_eq!(estado_inicial(true), "activo");
        assert_eq!(estado_inicial(false), "desactivado");
    }
}
