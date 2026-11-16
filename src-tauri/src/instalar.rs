//! Instalación de las integraciones: host de mensajería nativa, lanzador y autostart.

use crate::config;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Id de la extensión de Chrome (fijo en el contrato).
const EXTENSION_ID: &str = "lnmnebdbkffhodbjackjnkoegfdgcdle";
/// Nombre del host nativo.
const HOST: &str = "es.migas.host";
/// Icono de origen, dentro del manifiesto del crate.
const ICONO_ORIGEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/icons/128x128.png");

/// Instala en el home real y devuelve una línea por paso.
pub fn instalar() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_default();
    instalar_con_datos(&home, &exe, &config::dir_datos())
}

/// Desinstala del home real y devuelve una línea por paso.
pub fn desinstalar() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    desinstalar_con_datos(&home, &config::dir_datos())
}

/// `(host registrado, autostart registrado)` en el home real.
pub fn estado() -> (bool, bool) {
    let home = dirs::home_dir().unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_default();
    estado_en(&home, &exe)
}

/// Instala usando `home` y una carpeta de datos bajo el propio home (para pruebas).
pub fn instalar_en(home: &Path, exe: &Path) -> Vec<String> {
    instalar_con_datos(home, exe, &datos_en(home))
}

/// Desinstala usando `home` y la carpeta de datos de las pruebas.
pub fn desinstalar_en(home: &Path) -> Vec<String> {
    desinstalar_con_datos(home, &datos_en(home))
}

/// Comprueba host y autostart sin tocar el sistema.
pub fn estado_en(home: &Path, exe: &Path) -> (bool, bool) {
    let host_ok = manifiestos(home).iter().any(|ruta| host_valido(ruta, exe));
    let autostart = home.join(".config").join("autostart").join("migas.desktop");
    let autostart_ok = std::fs::read_to_string(autostart)
        .map(|texto| {
            let ruta = exe.to_string_lossy();
            let ruta_escapada = escapar(&ruta);
            texto.contains("--oculto")
                && (texto.contains(ruta.as_ref()) || texto.contains(&ruta_escapada))
        })
        .unwrap_or(false);
    (host_ok, autostart_ok)
}

fn datos_en(home: &Path) -> PathBuf {
    home.join(".local").join("share").join("migas")
}

fn instalar_con_datos(home: &Path, exe: &Path, datos: &Path) -> Vec<String> {
    let mut informe = Vec::new();
    let exe_texto = exe.to_string_lossy().to_string();

    for (ruta, etiqueta) in manifiestos(home)
        .into_iter()
        .zip(["Host nativo registrado", "Host de Chromium registrado"])
    {
        let contenido = json!({
            "name": HOST,
            "description": "migas",
            "path": exe_texto,
            "type": "stdio",
            "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
        });
        match escribir(&ruta, &contenido.to_string()) {
            Ok(()) => informe.push(format!("✓ {etiqueta} ({})", ruta.display())),
            Err(error) => informe.push(format!("✗ No se pudo registrar {etiqueta}: {error}")),
        }
    }
    // El segundo manifiesto solo se instala si Chromium existe; si no, `manifiestos` no lo
    // devuelve y el informe no habla de él.

    let icono = datos.join("migas.png");
    match copiar_icono(&icono) {
        Ok(()) => informe.push(format!("✓ Icono copiado ({})", icono.display())),
        Err(error) => informe.push(format!("✗ No se pudo copiar el icono: {error}")),
    }

    let launcher = home
        .join(".local")
        .join("share")
        .join("applications")
        .join("migas.desktop");
    let contenido = escritorio(&format!("Exec=\"{}\"", escapar(&exe_texto)), &icono);
    match escribir(&launcher, &contenido) {
        Ok(()) => informe.push(format!("✓ Lanzador creado ({})", launcher.display())),
        Err(error) => informe.push(format!("✗ No se pudo crear el lanzador: {error}")),
    }

    let autostart = home.join(".config").join("autostart").join("migas.desktop");
    let contenido = escritorio(
        &format!("Exec=\"{}\" --oculto", escapar(&exe_texto)),
        &icono,
    );
    match escribir(&autostart, &contenido) {
        Ok(()) => informe.push(format!("✓ Autostart creado ({})", autostart.display())),
        Err(error) => informe.push(format!("✗ No se pudo crear el autostart: {error}")),
    }
    informe
}

fn desinstalar_con_datos(home: &Path, datos: &Path) -> Vec<String> {
    let mut informe = Vec::new();
    for ruta in manifiestos(home) {
        match std::fs::remove_file(&ruta) {
            Ok(()) => informe.push(format!("✓ Host nativo eliminado ({})", ruta.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                informe.push(format!("· Host nativo no estaba ({})", ruta.display()));
            }
            Err(error) => informe.push(format!("✗ No se pudo quitar el host: {error}")),
        }
    }
    for ruta in [
        home.join(".local")
            .join("share")
            .join("applications")
            .join("migas.desktop"),
        home.join(".config").join("autostart").join("migas.desktop"),
        datos.join("migas.png"),
    ] {
        match std::fs::remove_file(&ruta) {
            Ok(()) => informe.push(format!("✓ Eliminado ({})", ruta.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                informe.push(format!("· No estaba ({})", ruta.display()));
            }
            Err(error) => informe.push(format!("✗ No se pudo eliminar: {error}")),
        }
    }
    informe
}

fn manifiestos(home: &Path) -> Vec<PathBuf> {
    let chrome = home
        .join(".config")
        .join("google-chrome")
        .join("NativeMessagingHosts")
        .join(format!("{HOST}.json"));
    let mut rutas = vec![chrome];
    if home.join(".config").join("chromium").exists() {
        rutas.push(
            home.join(".config")
                .join("chromium")
                .join("NativeMessagingHosts")
                .join(format!("{HOST}.json")),
        );
    }
    rutas
}

fn host_valido(ruta: &Path, exe: &Path) -> bool {
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return false;
    };
    let Ok(valor) = serde_json::from_str::<serde_json::Value>(&texto) else {
        return false;
    };
    valor.get("name").and_then(serde_json::Value::as_str) == Some(HOST)
        && valor.get("path").and_then(serde_json::Value::as_str)
            == Some(exe.to_string_lossy().as_ref())
        && valor
            .get("allowed_origins")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|origenes| {
                origenes.iter().any(|origen| {
                    origen.as_str() == Some(&format!("chrome-extension://{EXTENSION_ID}/"))
                })
            })
}

fn escribir(ruta: &Path, contenido: &str) -> std::io::Result<()> {
    if let Some(carpeta) = ruta.parent() {
        std::fs::create_dir_all(carpeta)?;
    }
    std::fs::write(ruta, contenido)
}

fn copiar_icono(destino: &Path) -> std::io::Result<()> {
    if let Some(carpeta) = destino.parent() {
        std::fs::create_dir_all(carpeta)?;
    }
    std::fs::copy(ICONO_ORIGEN, destino)?;
    Ok(())
}

fn escritorio(exec: &str, icono: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=migas\n\
         Comment=Diario de enlaces y notas\n\
         {exec}\n\
         Icon={}\n\
         Terminal=false\n\
         StartupWMClass=migas\n\
         Categories=Utility;\n",
        icono.display()
    )
}

fn escapar(texto: &str) -> String {
    let mut salida = String::with_capacity(texto.len());
    for caracter in texto.chars() {
        match caracter {
            '%' => salida.push_str("%%"),
            '"' => salida.push_str("\\\""),
            '`' => salida.push_str("\\`"),
            '$' => salida.push_str("\\$"),
            '\\' => salida.push_str("\\\\\\\\"),
            otro => salida.push(otro),
        }
    }
    salida
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instala_y_desinstala_en_un_home_temporal() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let home = tmp.path();
        std::fs::create_dir_all(home.join(".config").join("chromium")).expect("chromium");
        let exe = Path::new("/opt/migas/bin/migas");

        let lineas = instalar_en(home, exe);
        assert!(
            lineas.iter().all(|linea| !linea.starts_with('✗')),
            "errores: {lineas:?}"
        );
        let chrome = home
            .join(".config")
            .join("google-chrome")
            .join("NativeMessagingHosts")
            .join("es.migas.host.json");
        let chromium = home
            .join(".config")
            .join("chromium")
            .join("NativeMessagingHosts")
            .join("es.migas.host.json");
        assert!(chrome.exists());
        assert!(chromium.exists());
        let valor: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&chrome).expect("manifest"))
                .expect("json");
        assert_eq!(valor["name"], HOST);
        assert_eq!(valor["path"], exe.to_string_lossy().as_ref());
        assert_eq!(
            valor["allowed_origins"][0],
            format!("chrome-extension://{EXTENSION_ID}/")
        );

        let icono = datos_en(home).join("migas.png");
        assert!(icono.exists());
        assert!(std::fs::metadata(&icono).expect("icono").len() > 0);

        let launcher = home
            .join(".local")
            .join("share")
            .join("applications")
            .join("migas.desktop");
        let autostart = home.join(".config").join("autostart").join("migas.desktop");
        let texto = std::fs::read_to_string(&launcher).expect("lanzador");
        assert!(texto.contains("Exec=\"/opt/migas/bin/migas\""));
        assert!(texto.contains("StartupWMClass=migas"));
        let texto = std::fs::read_to_string(&autostart).expect("autostart");
        assert!(texto.contains("--oculto"));

        assert_eq!(estado_en(home, exe), (true, true));

        let lineas = desinstalar_en(home);
        assert!(
            lineas.iter().all(|linea| !linea.starts_with('✗')),
            "errores: {lineas:?}"
        );
        assert!(!chrome.exists());
        assert!(!chromium.exists());
        assert!(!launcher.exists());
        assert!(!autostart.exists());
        assert!(!icono.exists());
        assert_eq!(estado_en(home, exe), (false, false));
    }

    #[test]
    fn sin_chromium_solo_instala_el_host_de_chrome() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let lineas = instalar_en(tmp.path(), Path::new("/opt/migas/bin/migas"));
        assert!(
            lineas
                .iter()
                .any(|linea| linea.contains("Host nativo registrado"))
        );
        assert!(
            !lineas
                .iter()
                .any(|linea| linea.contains("Host de Chromium registrado"))
        );
        assert!(
            !tmp.path()
                .join(".config")
                .join("chromium")
                .join("NativeMessagingHosts")
                .join("es.migas.host.json")
                .exists()
        );
    }

    #[test]
    fn escapa_exec_y_valida_los_desktop() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let exe = Path::new("/opt/migas prueba/bin/migas%prueba");
        let lineas = instalar_en(tmp.path(), exe);
        assert!(
            lineas.iter().all(|linea| !linea.starts_with('✗')),
            "errores: {lineas:?}"
        );
        let launcher = tmp
            .path()
            .join(".local")
            .join("share")
            .join("applications")
            .join("migas.desktop");
        let autostart = tmp
            .path()
            .join(".config")
            .join("autostart")
            .join("migas.desktop");
        let texto = std::fs::read_to_string(&launcher).expect("lanzador");
        assert!(
            texto.contains("Exec=\"/opt/migas prueba/bin/migas%%prueba\""),
            "{texto}"
        );
        let texto_autostart = std::fs::read_to_string(&autostart).expect("autostart");
        assert!(
            texto_autostart.contains("Exec=\"/opt/migas prueba/bin/migas%%prueba\" --oculto"),
            "{texto_autostart}"
        );
        assert_eq!(estado_en(tmp.path(), exe), (true, true));

        for ruta in [&launcher, &autostart] {
            match std::process::Command::new("desktop-file-validate")
                .arg(ruta)
                .status()
            {
                Ok(estado) => assert!(estado.success(), "desktop inválido: {}", ruta.display()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("no se pudo ejecutar desktop-file-validate: {error}"),
            }
        }
    }

    #[test]
    fn escapa_la_gramatica_de_exec() {
        assert_eq!(escapar("a%b\"c`d$e\\f"), "a%%b\\\"c\\`d\\$e\\\\\\\\f");
    }

    #[test]
    fn escapa_barra_invertida_y_valida_desktop() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let exe = Path::new("/opt/migas prueba/bin/migas\\barra");
        let lineas = instalar_en(tmp.path(), exe);
        assert!(
            lineas.iter().all(|linea| !linea.starts_with('✗')),
            "errores: {lineas:?}"
        );
        let launcher = tmp
            .path()
            .join(".local")
            .join("share")
            .join("applications")
            .join("migas.desktop");
        let autostart = tmp
            .path()
            .join(".config")
            .join("autostart")
            .join("migas.desktop");
        let cuatro = "\\".repeat(4);
        for ruta in [&launcher, &autostart] {
            let texto = std::fs::read_to_string(ruta).expect("desktop");
            assert!(
                texto.contains(&format!("migas{cuatro}barra")),
                "falta el escape doble en {}: {texto}",
                ruta.display()
            );
            match std::process::Command::new("desktop-file-validate")
                .arg(ruta)
                .status()
            {
                Ok(estado) => assert!(estado.success(), "desktop inválido: {}", ruta.display()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("no se pudo ejecutar desktop-file-validate: {error}"),
            }
        }
        assert_eq!(estado_en(tmp.path(), exe), (true, true));
    }
}
