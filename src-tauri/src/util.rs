//! Utilidades compartidas: fechas en español y filtro de palabras prohibidas.

use chrono::{DateTime, Datelike, FixedOffset, Local};

const MESES: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
];

/// Parsea RFC 3339 y también el formato de Jira (`2026-10-16T14:02:11.123+0200`).
pub fn parse_fecha(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()
}

/// `15-oct` en hora local.
pub fn fecha_corta(s: &str) -> String {
    match parse_fecha(s) {
        Some(d) => {
            let l = d.with_timezone(&Local);
            format!("{}-{}", l.day(), MESES[l.month0() as usize])
        }
        None => s.to_string(),
    }
}

/// Palabras que delatan una firma de agente o una mención a IA. Se comparan sin mayúsculas.
const PROHIBIDAS: [&str; 11] = [
    "claude",
    "inteligencia artificial",
    "generated",
    "co-authored",
    "🤖",
    "asistente",
    "modelo de lenguaje",
    "chatgpt",
    "deepseek",
    "ollama",
    "openai",
];

/// `true` si el texto contiene alguna palabra prohibida o la palabra suelta «IA».
pub fn tiene_prohibidas(texto: &str) -> bool {
    let t = texto.to_lowercase();
    PROHIBIDAS.iter().any(|p| t.contains(p))
        || t.split(|c: char| !c.is_alphanumeric()).any(|w| w == "ia")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fechas() {
        assert!(parse_fecha("2026-10-16T14:02:11.123+0200").is_some());
        assert!(parse_fecha("2026-10-13T17:53:07.691+02:00").is_some());
        assert_eq!(fecha_corta("2026-10-15T10:00:00+02:00"), "15-oct");
    }

    #[test]
    fn prohibidas() {
        assert!(tiene_prohibidas("🤖 Generated with Claude"));
        assert!(tiene_prohibidas("Redactado por IA para ti"));
        assert!(!tiene_prohibidas(
            "Pedido el despliegue a DESARROLLO: https://x/y"
        ));
        assert!(!tiene_prohibidas("Se ha pedido la media"));
    }
}
