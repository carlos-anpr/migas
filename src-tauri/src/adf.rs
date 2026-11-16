//! Atlassian Document Format ↔ texto plano. Lo usan la lectura de Jira (ADF → texto) y la
//! publicación de comentarios (texto → ADF). La vista previa del frontend sigue las mismas
//! reglas que `texto_a_adf`: párrafos por línea en blanco, saltos de línea, URLs como enlace
//! y `código` entre comillas invertidas.

use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

static RE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s<>()\[\]]+[^\s<>()\[\].,;:!?»”']").unwrap());

/// Texto plano de un documento ADF. Los enlaces (`inlineCard`, marca `link`) aportan su URL
/// para que la extracción de referencias encuentre `work_items/…` o `merge_requests/…`.
pub fn adf_a_texto(doc: &Value) -> String {
    let mut out = String::new();
    recorrer(doc, &mut out);
    out.trim().to_string()
}

fn recorrer(n: &Value, out: &mut String) {
    let tipo = n.get("type").and_then(Value::as_str).unwrap_or("");
    match tipo {
        "text" => {
            let t = n.get("text").and_then(Value::as_str).unwrap_or("");
            out.push_str(t);
            if let Some(marks) = n.get("marks").and_then(Value::as_array) {
                for m in marks {
                    if m.get("type").and_then(Value::as_str) == Some("link")
                        && let Some(href) = m.pointer("/attrs/href").and_then(Value::as_str)
                        && href != t
                    {
                        out.push_str(" (");
                        out.push_str(href);
                        out.push(')');
                    }
                }
            }
        }
        "inlineCard" | "blockCard" | "embedCard" => {
            if let Some(url) = n.pointer("/attrs/url").and_then(Value::as_str) {
                out.push_str(url);
            }
        }
        "mention" => {
            if let Some(t) = n.pointer("/attrs/text").and_then(Value::as_str) {
                out.push_str(t);
            }
        }
        "hardBreak" => out.push('\n'),
        _ => {}
    }
    if let Some(hijos) = n.get("content").and_then(Value::as_array) {
        for h in hijos {
            recorrer(h, out);
        }
        if matches!(
            tipo,
            "paragraph" | "heading" | "listItem" | "codeBlock" | "blockquote" | "tableRow"
        ) {
            out.push('\n');
        }
    }
}

/// Documento ADF de un comentario escrito en texto plano.
pub fn texto_a_adf(texto: &str) -> Value {
    let normal = texto.replace("\r\n", "\n");
    let parrafos: Vec<Value> = normal
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut contenido = Vec::new();
            for (i, linea) in p.lines().enumerate() {
                if i > 0 {
                    contenido.push(json!({"type": "hardBreak"}));
                }
                contenido.extend(inline(linea));
            }
            json!({"type": "paragraph", "content": contenido})
        })
        .collect();
    json!({"type": "doc", "version": 1, "content": parrafos})
}

/// Trocea una línea en nodos de texto: `código` → marca code, URLs → marca link.
fn inline(linea: &str) -> Vec<Value> {
    let mut nodos = Vec::new();
    for (i, trozo) in linea.split('`').enumerate() {
        if trozo.is_empty() {
            continue;
        }
        // Los índices impares están entre comillas invertidas (si el número de ` es par).
        if i % 2 == 1 && linea.matches('`').count().is_multiple_of(2) {
            nodos.push(json!({"type": "text", "text": trozo, "marks": [{"type": "code"}]}));
            continue;
        }
        let mut ultimo = 0;
        for m in RE_URL.find_iter(trozo) {
            if m.start() > ultimo {
                nodos.push(json!({"type": "text", "text": &trozo[ultimo..m.start()]}));
            }
            nodos.push(json!({"type": "text", "text": m.as_str(), "marks": [{"type": "link", "attrs": {"href": m.as_str()}}]}));
            ultimo = m.end();
        }
        if ultimo < trozo.len() {
            nodos.push(json!({"type": "text", "text": &trozo[ultimo..]}));
        }
    }
    nodos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_texto_enlaces_y_tarjetas() {
        let doc = json!({"type":"doc","version":1,"content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"Nueva WI "},
                {"type":"inlineCard","attrs":{"url":"https://gitlab.com/equipo/despliegues/-/work_items/14"}},
                {"type":"hardBreak"},
                {"type":"text","text":"MR","marks":[{"type":"link","attrs":{"href":"https://gitlab.com/a/b/-/merge_requests/35"}}]}
            ]},
            {"type":"paragraph","content":[{"type":"text","text":"Fin"}]}
        ]});
        let t = adf_a_texto(&doc);
        assert!(t.contains("work_items/14"));
        assert!(t.contains("MR (https://gitlab.com/a/b/-/merge_requests/35)"));
        assert!(t.ends_with("Fin"));
    }

    #[test]
    fn escribe_parrafos_saltos_enlaces_y_codigo() {
        let v = texto_a_adf("Pedido el despliegue: https://x.es/w/1.\nRama `feat/a`\n\nSegundo");
        let ps = v["content"].as_array().unwrap();
        assert_eq!(ps.len(), 2);
        let c = ps[0]["content"].as_array().unwrap();
        assert_eq!(c[1]["marks"][0]["attrs"]["href"], "https://x.es/w/1");
        assert_eq!(c[2]["text"], ".");
        assert!(c.iter().any(|n| n["type"] == "hardBreak"));
        assert!(
            c.iter()
                .any(|n| n["text"] == "feat/a" && n["marks"][0]["type"] == "code")
        );
    }
}
