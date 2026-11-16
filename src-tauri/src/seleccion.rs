//! Lectura de la selección PRIMARY y del portapapeles, con `wl-paste` y `xclip` de reserva.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Tiempo máximo total por comando (incluye la lectura de stdout).
const TIMEOUT: Duration = Duration::from_millis(800);
/// Tope de bytes leídos del proceso.
const MAX_BYTES: usize = 80_000;
/// Tope de caracteres devuelto (el contrato de la ventana rápida).
const MAX_TEXTO: usize = 20_000;

/// Texto seleccionado (PRIMARY). `None` si no hay o no se puede leer.
pub fn primaria() -> Option<String> {
    leer("wl-paste", &["--primary", "--no-newline", "--type", "text"])
        .or_else(|| leer("xclip", &["-o", "-selection", "primary"]))
}

/// Contenido del portapapeles. `None` si no hay o no se puede leer.
pub fn portapapeles() -> Option<String> {
    leer("wl-paste", &["--no-newline", "--type", "text"])
        .or_else(|| leer("xclip", &["-o", "-selection", "clipboard"]))
}

/// Huellas de la última selección y portapapeles ya vistos por la app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vistos {
    pub primaria: Option<u64>,
    pub portapapeles: Option<u64>,
}

pub fn huella(texto: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    texto.hash(&mut hasher);
    hasher.finish()
}

/// Lee ambos al arrancar: lo que ya estaba no debe ofrecerse como nuevo.
pub fn vistos_iniciales() -> Vistos {
    Vistos {
        primaria: primaria().as_deref().map(huella),
        portapapeles: portapapeles().as_deref().map(huella),
    }
}

/// Devuelve solo lo que cambió respecto a `vistos` y actualiza las huellas.
/// El mismo contenido dos veces no se ofrece la segunda vez.
pub fn solo_nuevos(
    primaria: Option<String>,
    portapapeles: Option<String>,
    vistos: &mut Vistos,
) -> (Option<String>, Option<String>) {
    let huella_primaria = primaria.as_deref().map(huella);
    let huella_portapapeles = portapapeles.as_deref().map(huella);
    let nueva_primaria = if huella_primaria != vistos.primaria {
        primaria
    } else {
        None
    };
    let nuevo_portapapeles = if huella_portapapeles != vistos.portapapeles {
        portapapeles
    } else {
        None
    };
    vistos.primaria = huella_primaria;
    vistos.portapapeles = huella_portapapeles;
    (nueva_primaria, nuevo_portapapeles)
}

fn leer(programa: &str, argumentos: &[&str]) -> Option<String> {
    ejecutar(programa, argumentos).and_then(limpiar)
}

/// Recorta, quita el vacío y limita a 20 000 caracteres.
fn limpiar(texto: String) -> Option<String> {
    let recortado = texto.trim();
    if recortado.is_empty() {
        return None;
    }
    Some(recortado.chars().take(MAX_TEXTO).collect())
}

/// Ejecuta sin shell y con un plazo total de 800 ms, incluida la lectura.
/// Nunca hace `join()` del lector: usa un canal con `recv_timeout`.
fn ejecutar(programa: &str, argumentos: &[&str]) -> Option<String> {
    let mut hijo = Command::new(programa)
        .args(argumentos)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let salida = hijo.stdout.take()?;
    let (emisor, receptor) = mpsc::channel();
    std::thread::Builder::new()
        .name("migas-seleccion-lector".into())
        .spawn(move || {
            let mut salida = salida;
            let mut datos = Vec::new();
            let _ = salida
                .by_ref()
                .take(MAX_BYTES as u64)
                .read_to_end(&mut datos);
            // Si hemos llegado al tope, el productor puede terminar con SIGPIPE; la
            // salida parcial sigue siendo válida.
            let cortado = datos.len() >= MAX_BYTES;
            let _ = emisor.send((datos, cortado));
        })
        .ok()?;

    let limite = Instant::now() + TIMEOUT;
    loop {
        match hijo.try_wait() {
            Ok(Some(estado)) => {
                let restante = limite.saturating_duration_since(Instant::now());
                let datos = receptor.recv_timeout(restante).ok();
                return match datos {
                    Some((bytes, cortado)) if estado.success() || cortado => {
                        Some(String::from_utf8_lossy(&bytes).into_owned())
                    }
                    _ => None,
                };
            }
            Ok(None) => {
                if Instant::now() >= limite {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                let _ = hijo.kill();
                let _ = hijo.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limpia_y_recorta() {
        assert_eq!(limpiar("  hola\n".into()), Some("hola".into()));
        assert_eq!(limpiar(" \n\t ".into()), None);
        let largo = limpiar("a".repeat(100_000)).expect("texto");
        assert_eq!(largo.chars().count(), MAX_TEXTO);
        let largo = limpiar("á".repeat(50_000)).expect("texto");
        assert_eq!(largo.chars().count(), MAX_TEXTO);
    }

    #[test]
    fn lee_la_salida_normal() {
        assert_eq!(
            ejecutar("sh", &["-c", "printf hola"]),
            Some("hola".to_string())
        );
    }

    #[test]
    fn acepta_salida_parcial_tras_el_corte() {
        let salida = ejecutar("sh", &["-c", "head -c 500000 /dev/zero | tr '\\0' a"])
            .and_then(limpiar)
            .expect("salida parcial");
        assert_eq!(salida.chars().count(), MAX_TEXTO);
    }

    #[test]
    fn recorta_a_veinte_mil_caracteres() {
        let salida = ejecutar("sh", &["-c", "yes a | head -c 20001"])
            .and_then(limpiar)
            .expect("salida");
        assert_eq!(salida.chars().count(), MAX_TEXTO);
    }

    #[test]
    fn rechaza_un_error_sin_corte() {
        assert_eq!(ejecutar("sh", &["-c", "printf hola; exit 1"]), None);
    }

    #[test]
    fn solo_ofrece_contenido_nuevo() {
        let mut vistos = Vistos::default();
        let (primaria, portapapeles) = solo_nuevos(Some("selección".into()), None, &mut vistos);
        assert_eq!(primaria.as_deref(), Some("selección"));
        assert!(portapapeles.is_none());
        // El mismo contenido dos veces: la segunda no se ofrece.
        let (primaria, portapapeles) = solo_nuevos(Some("selección".into()), None, &mut vistos);
        assert!(primaria.is_none());
        assert!(portapapeles.is_none());

        let (primaria, portapapeles) = solo_nuevos(
            Some("selección".into()),
            Some("portapapeles".into()),
            &mut vistos,
        );
        assert!(primaria.is_none());
        assert_eq!(portapapeles.as_deref(), Some("portapapeles"));
        let (primaria, portapapeles) = solo_nuevos(
            Some("otra".into()),
            Some("portapapeles".into()),
            &mut vistos,
        );
        assert_eq!(primaria.as_deref(), Some("otra"));
        assert!(portapapeles.is_none());
    }

    #[test]
    fn no_espera_al_descendiente_que_hereda_stdout() {
        let inicio = Instant::now();
        let salida = ejecutar("sh", &["-c", "(sleep 2) & printf hola"]);
        let duracion = inicio.elapsed();
        assert!(duracion < Duration::from_millis(1000), "tardó {duracion:?}");
        // El hijo directo sale, pero el descendiente mantiene stdout abierto y no se
        // puede completar la lectura dentro del plazo.
        assert_eq!(salida, None);
    }
}
