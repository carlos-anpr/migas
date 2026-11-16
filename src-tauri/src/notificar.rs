//! Notificación de escritorio con `notify-send` (sin shell y sin bloquear).

/// Notifica en el escritorio. En pruebas no hace nada (no lanza procesos).
#[cfg(test)]
pub fn notificar(_titulo: &str, _cuerpo: &str) {}

/// Notifica en el escritorio. Errores, solo a stderr.
#[cfg(not(test))]
pub fn notificar(titulo: &str, cuerpo: &str) {
    use std::process::{Command, Stdio};

    let mut comando = Command::new("notify-send");
    comando.arg("-a").arg("migas");
    if let Some(icono) = icono() {
        comando.arg("-i").arg(icono);
    }
    comando.arg("--").arg(titulo).arg(cuerpo);
    comando
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match comando.spawn() {
        // El proceso se cosecha en un hilo aparte para no dejar zombis ni bloquear al llamante.
        Ok(mut hijo) => {
            std::thread::spawn(move || {
                let _ = hijo.wait();
            });
        }
        Err(error) => eprintln!("migas notificar: {error}"),
    }
}

/// Icono de la app, si está instalado en alguna de las rutas habituales.
#[cfg(not(test))]
fn icono() -> Option<std::path::PathBuf> {
    // F3 instala el icono junto a la BD; en desarrollo se usa el del manifiesto.
    let candidatos = vec![
        crate::config::dir_datos().join("migas.png"),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join("128.png"),
    ];
    candidatos.into_iter().find(|ruta| ruta.exists())
}

#[cfg(test)]
mod tests {
    #[test]
    fn en_pruebas_no_lanza_nada() {
        super::notificar("migas", "prueba");
    }
}
