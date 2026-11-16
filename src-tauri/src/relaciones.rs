//! Relaciones automáticas y manuales entre entradas, con motivo y peso.

use crate::error::{Error, Resultado};
use crate::model::Relacion;
use crate::recurso;
use crate::{consultas, util};
use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeSet;
const MESES: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
];

/// Entradas relacionadas con `id`, con motivo y peso. Orden: peso desc, luego más reciente.
pub fn relacionadas(conn: &Connection, id: i64, limite: u32) -> Resultado<Vec<Relacion>> {
    if limite == 0 {
        return Ok(Vec::new());
    }
    let fuente = base(conn, id)?;
    let mias = entidades_de(conn, id)?;

    let mut encontradas: Vec<(u32, i64, Relacion)> = Vec::new();
    for candidata in candidatas(conn, id)? {
        let vista = consultas::entrada_vista_base(conn, candidata.id)?;
        let suyas = entidades_de(conn, candidata.id)?;
        let manual = esta_en(conn, "enlace_manual", id, candidata.id)?;
        let descartada = esta_en(conn, "descarte", id, candidata.id)?;
        if descartada && !manual {
            continue;
        }

        let mut peso: u32 = 0;
        let mut motivos: Vec<(u32, String)> = Vec::new();
        if manual {
            peso += 100;
            motivos.push((100, "Enlazada a mano".into()));
        }

        let mut compartidas: BTreeSet<String> = mias.intersection(&suyas).cloned().collect();
        let mismo_recurso = fuente.recurso.is_some() && fuente.recurso == candidata.recurso;
        if mismo_recurso && let Some(clave) = &fuente.recurso {
            compartidas.remove(clave);
            peso += 90;
            motivos.push((90, format!("La misma, el {}", dia_corto(&candidata.dia))));
        }

        let mut proyectos: Vec<String> = Vec::new();
        let mut hay_app_compartida = false;
        for valor in compartidas {
            if let Some(app) = valor.strip_prefix("app:") {
                hay_app_compartida = true;
                peso += 20;
                motivos.push((20, format!("App {app}")));
            } else if let Some(proyecto) = valor.strip_prefix("proyecto:") {
                proyectos.push(proyecto.to_string());
            } else if valor.starts_with("jira:")
                || valor.starts_with("fs:")
                || valor.starts_with("mr:")
                || valor.starts_with("issue:")
            {
                peso += 40;
                motivos.push((40, recurso::visible(&valor)));
            } else if let Some(numero) = valor.strip_prefix("wi:") {
                peso += 40;
                motivos.push((40, format!("WI {numero}")));
            }
        }
        // El proyecto solo suma si no hay una app compartida, que es el motivo más fino.
        if !hay_app_compartida {
            for proyecto in proyectos {
                peso += 10;
                motivos.push((10, format!("Proyecto {proyecto}")));
            }
        }

        if peso == 0 {
            continue;
        }
        motivos.sort_by_key(|motivo| std::cmp::Reverse(motivo.0));
        let mut textos: Vec<String> = Vec::new();
        for (_, texto) in motivos {
            if !textos.contains(&texto) {
                textos.push(texto);
            }
        }
        textos.truncate(2);
        let instante = util::parse_fecha(&candidata.creada)
            .map(|fecha| fecha.timestamp())
            .unwrap_or(0);
        encontradas.push((
            peso,
            instante,
            Relacion {
                entrada: vista,
                motivos: textos,
                peso,
                manual,
            },
        ));
    }

    encontradas.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(encontradas
        .into_iter()
        .take(limite as usize)
        .map(|(_, _, relacion)| relacion)
        .collect())
}

/// Número de entradas relacionadas (mismo criterio que [`relacionadas`], sin límite).
pub fn contar(conn: &Connection, id: i64) -> Resultado<u32> {
    Ok(u32::try_from(candidatas(conn, id)?.len()).unwrap_or(u32::MAX))
}

struct Base {
    recurso: Option<String>,
}

struct Candidata {
    id: i64,
    recurso: Option<String>,
    dia: String,
    creada: String,
}

fn base(conn: &Connection, id: i64) -> Resultado<Base> {
    let fila = conn
        .query_row(
            "SELECT recurso, dia, creada FROM entrada WHERE id = ?1",
            [id],
            |fila| {
                Ok((
                    fila.get::<_, Option<String>>(0)?,
                    fila.get::<_, String>(1)?,
                    fila.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((recurso, _dia, _creada)) = fila else {
        return Err(Error::NoExiste(format!("la entrada {id}")));
    };
    Ok(Base { recurso })
}

/// Candidatas: vivas, distintas de `id`, que comparten alguna entidad o están enlazadas a mano.
/// Se excluyen las descartadas salvo que el enlace sea manual.
fn candidatas(conn: &Connection, id: i64) -> Resultado<Vec<Candidata>> {
    let mut consulta = conn.prepare(
        "WITH mias(valor) AS (
             SELECT valor FROM entidad WHERE entrada_id = ?1
             UNION
             SELECT re.valor FROM recurso_entidad re
              WHERE re.clave = (SELECT recurso FROM entrada WHERE id = ?1)
         )
         SELECT e.id, e.recurso, e.dia, e.creada
           FROM entrada e
          WHERE e.borrada IS NULL
            AND e.id <> ?1
            AND (
                EXISTS (SELECT 1 FROM enlace_manual m
                         WHERE (m.a = ?1 AND m.b = e.id) OR (m.a = e.id AND m.b = ?1))
                OR (
                    NOT EXISTS (SELECT 1 FROM descarte d
                                 WHERE (d.a = ?1 AND d.b = e.id) OR (d.a = e.id AND d.b = ?1))
                    AND (
                        EXISTS (SELECT 1 FROM entidad x
                                 WHERE x.entrada_id = e.id
                                   AND x.valor IN (SELECT valor FROM mias))
                        OR EXISTS (SELECT 1 FROM recurso_entidad x
                                    WHERE x.clave = e.recurso
                                      AND x.valor IN (SELECT valor FROM mias))
                    )
                )
            )
          ORDER BY e.id DESC",
    )?;
    let filas = consulta.query_map([id], |fila| {
        Ok(Candidata {
            id: fila.get(0)?,
            recurso: fila.get(1)?,
            dia: fila.get(2)?,
            creada: fila.get(3)?,
        })
    })?;
    Ok(filas.collect::<Result<Vec<Candidata>, _>>()?)
}

fn entidades_de(conn: &Connection, id: i64) -> Resultado<BTreeSet<String>> {
    let mut consulta = conn.prepare(
        "SELECT valor FROM entidad WHERE entrada_id = ?1
         UNION
         SELECT re.valor FROM recurso_entidad re
          WHERE re.clave = (SELECT recurso FROM entrada WHERE id = ?1)",
    )?;
    let filas = consulta.query_map([id], |fila| fila.get::<_, String>(0))?;
    Ok(filas.collect::<Result<BTreeSet<String>, _>>()?)
}

fn esta_en(conn: &Connection, tabla: &str, a: i64, b: i64) -> Resultado<bool> {
    // `tabla` solo puede ser una de las dos constantes internas.
    let sql = match tabla {
        "enlace_manual" => {
            "SELECT 1 FROM enlace_manual WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)"
        }
        _ => "SELECT 1 FROM descarte WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)",
    };
    Ok(conn
        .query_row(sql, params![a, b], |_| Ok(()))
        .optional()?
        .is_some())
}

fn dia_corto(dia: &str) -> String {
    match NaiveDate::parse_from_str(dia, "%Y-%m-%d") {
        Ok(fecha) => format!("{}-{}", fecha.day(), MESES[fecha.month0() as usize]),
        Err(_) => dia.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captura::capturar;
    use crate::db;
    use crate::model::{Ajustes, NuevaEntrada, Origen, TipoEntrada};
    use chrono::{DateTime, Local, TimeZone};
    use rusqlite::Connection;

    fn bd(dir: &std::path::Path) -> Connection {
        db::abrir(&dir.join("migas.db")).expect("BD de prueba")
    }

    fn cuando(dia: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, dia, 10, 0, 0).unwrap()
    }

    fn enlace(conn: &Connection, url: &str, dia: u32) -> i64 {
        capturar(
            conn,
            &NuevaEntrada {
                tipo: TipoEntrada::Enlace,
                origen: Origen::Navegador,
                url: Some(url.into()),
                titulo: None,
                seleccion: None,
                nota: None,
            },
            &Ajustes::default(),
            cuando(dia),
        )
        .expect("captura enlace")
        .id
    }

    fn texto(conn: &Connection, texto: &str, dia: u32) -> i64 {
        capturar(
            conn,
            &NuevaEntrada {
                tipo: TipoEntrada::Texto,
                origen: Origen::Global,
                url: None,
                titulo: None,
                seleccion: Some(texto.into()),
                nota: None,
            },
            &Ajustes::default(),
            cuando(dia),
        )
        .expect("captura texto")
        .id
    }

    fn motivos(conn: &Connection, id: i64) -> Vec<String> {
        relacionadas(conn, id, 12).expect("relacionadas")[0]
            .motivos
            .clone()
    }

    #[test]
    fn mismo_recurso_en_otro_dia() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let vieja = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 4);
        let nueva = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 5);
        let relaciones = relacionadas(&conn, nueva, 12).expect("relacionadas");
        assert_eq!(relaciones.len(), 1);
        assert_eq!(relaciones[0].peso, 90);
        assert!(!relaciones[0].manual);
        assert_eq!(relaciones[0].entrada.id, vieja);
        assert_eq!(
            relaciones[0].motivos,
            vec!["La misma, el 4-nov".to_string()]
        );
    }

    #[test]
    fn motivo_de_cada_tipo_de_entidad() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());

        // jira: una entrada con el recurso y otra que lo menciona.
        let jira = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 4);
        let mencion = texto(&conn, "hablando de PROJ-1896", 4);
        assert_eq!(motivos(&conn, mencion), vec!["PROJ-1896".to_string()]);
        assert_eq!(relacionadas(&conn, mencion, 12).expect("rel")[0].peso, 40);
        assert!(
            relacionadas(&conn, jira, 12)
                .expect("rel")
                .iter()
                .any(|r| r.entrada.id == mencion)
        );

        // fs
        enlace(&conn, "https://empresa.freshservice.com/a/tickets/45600", 4);
        let fs_texto = texto(&conn, "ticket FS#45600 pendiente", 4);
        assert_eq!(motivos(&conn, fs_texto), vec!["FS#45600".to_string()]);

        // wi
        enlace(
            &conn,
            "https://gitlab.com/equipo/despliegues/-/work_items/1234",
            4,
        );
        let wi_texto = texto(&conn, "desplegar WI 1234", 4);
        assert_eq!(motivos(&conn, wi_texto), vec!["WI 1234".to_string()]);

        // mr
        enlace(
            &conn,
            "https://gitlab.com/grupo/proy/-/merge_requests/39",
            4,
        );
        let mr_texto = texto(
            &conn,
            "revisado https://gitlab.com/grupo/proy/-/merge_requests/39",
            4,
        );
        assert_eq!(motivos(&conn, mr_texto), vec!["!39".to_string()]);
    }

    #[test]
    fn motivo_issue() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let con_recurso = enlace(&conn, "https://gitlab.com/a/b/-/issues/7", 4);
        let otra = texto(&conn, "otra cosa", 4);
        // El texto libre no extrae issues; se simula la entidad que dejaría el seguimiento.
        conn.execute(
            "INSERT INTO entidad (entrada_id, valor, fuente) VALUES (?1, 'issue:a/b#7', 'texto')",
            [otra],
        )
        .expect("entidad");
        assert_eq!(motivos(&conn, otra), vec!["#7".to_string()]);
        assert!(relacionadas(&conn, otra, 12).expect("rel")[0].entrada.id == con_recurso);
    }

    #[test]
    fn app_compartida_en_distintas_mrs() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let uno = enlace(
            &conn,
            "https://gitlab.com/grupo/informes-web/-/merge_requests/1",
            4,
        );
        let dos = enlace(
            &conn,
            "https://gitlab.com/grupo/informes-web/-/merge_requests/2",
            4,
        );
        let relaciones = relacionadas(&conn, dos, 12).expect("relacionadas");
        let relacion = relaciones
            .iter()
            .find(|relacion| relacion.entrada.id == uno)
            .expect("relación");
        assert_eq!(relacion.peso, 20);
        assert_eq!(relacion.motivos, vec!["App informes-web".to_string()]);
    }

    #[test]
    fn proyecto_de_repo_wi_no_relaciona() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let uno = enlace(&conn, "https://gitlab.com/equipo/despliegues/-/issues/1", 4);
        let dos = enlace(&conn, "https://gitlab.com/equipo/despliegues/-/issues/2", 4);
        assert!(
            relacionadas(&conn, dos, 12)
                .expect("relacionadas")
                .is_empty(),
            "el repo de WI no puede ser motivo de relación"
        );
        assert!(uno != dos);
    }

    #[test]
    fn dos_wis_se_relacionan_solo_si_comparten_app() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let uno = enlace(
            &conn,
            "https://gitlab.com/equipo/despliegues/-/work_items/101",
            4,
        );
        let dos = enlace(
            &conn,
            "https://gitlab.com/equipo/despliegues/-/work_items/102",
            4,
        );
        let clave = |id: i64| -> String {
            conn.query_row("SELECT recurso FROM entrada WHERE id = ?1", [id], |fila| {
                fila.get(0)
            })
            .expect("recurso")
        };
        let app_a = crate::entidades::app_de_wi("## Aplicación\n\napp-a (v1)").expect("app A");
        let app_b = crate::entidades::app_de_wi("## Aplicación\n\napp-b (v1)").expect("app B");
        conn.execute(
            "INSERT INTO recurso_entidad (clave, valor) VALUES (?1, ?3), (?2, ?4)",
            params![
                clave(uno),
                clave(dos),
                format!("app:{app_a}"),
                format!("app:{app_b}")
            ],
        )
        .expect("apps distintas");
        assert!(
            relacionadas(&conn, dos, 12)
                .expect("relacionadas")
                .is_empty(),
            "apps distintas no se relacionan"
        );

        conn.execute(
            "UPDATE recurso_entidad SET valor = ?2
              WHERE clave = ?1 AND valor = ?3",
            params![clave(dos), format!("app:{app_a}"), format!("app:{app_b}")],
        )
        .expect("misma app");
        let relaciones = relacionadas(&conn, dos, 12).expect("relacionadas");
        let relacion = relaciones
            .iter()
            .find(|relacion| relacion.entrada.id == uno)
            .expect("relación por app");
        assert_eq!(relacion.peso, 20);
        assert_eq!(relacion.motivos, vec![format!("App {app_a}")]);
    }

    #[test]
    fn descarte_oculta_y_el_enlace_manual_gana() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let a = texto(&conn, "PROJ-1896 contexto", 4);
        let b = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 4);
        assert!(!relacionadas(&conn, a, 12).expect("rel").is_empty());
        crate::consultas::descartar(&conn, a, b).expect("descarta");
        assert!(relacionadas(&conn, a, 12).expect("rel").is_empty());
        assert_eq!(contar(&conn, a).expect("cuenta"), 0);

        crate::consultas::enlazar(&conn, b, a, cuando(4)).expect("enlaza");
        let relaciones = relacionadas(&conn, a, 12).expect("rel");
        assert_eq!(relaciones.len(), 1);
        assert!(relaciones[0].manual);
        // El enlace manual suma 100 y la entidad Jira compartida otros 40.
        assert_eq!(relaciones[0].peso, 140);
        assert_eq!(
            relaciones[0].motivos,
            vec!["Enlazada a mano".to_string(), "PROJ-1896".to_string()]
        );
        assert_eq!(contar(&conn, a).expect("cuenta"), 1);
    }

    #[test]
    fn excluye_la_propia_y_las_borradas() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let a = texto(&conn, "PROJ-1896", 4);
        let b = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 4);
        let propias: Vec<i64> = relacionadas(&conn, a, 12)
            .expect("rel")
            .into_iter()
            .map(|relacion| relacion.entrada.id)
            .collect();
        assert!(!propias.contains(&a));
        crate::consultas::borrar(&conn, b, cuando(4)).expect("borra");
        assert!(relacionadas(&conn, a, 12).expect("rel").is_empty());
    }

    #[test]
    fn orden_por_peso_y_recencia() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        // El vocabulario de apps alimenta las entidades de texto.
        conn.execute(
            "INSERT INTO vocabulario (app, fuente) VALUES ('informes-web', 'ajustes')",
            [],
        )
        .expect("vocabulario");
        // Fuente: un texto que comparte jira y app con varias candidatas.
        let fuente = texto(&conn, "PROJ-1896 y informes-web", 4);
        let manual = texto(&conn, "otra cualquiera", 3);
        crate::consultas::enlazar(&conn, fuente, manual, cuando(4)).expect("enlaza");
        let jira = enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 5);
        let app = enlace(
            &conn,
            "https://gitlab.com/grupo/informes-web/-/merge_requests/1",
            6,
        );
        let app_vieja = enlace(
            &conn,
            "https://gitlab.com/grupo/informes-web/-/merge_requests/2",
            2,
        );
        let relaciones = relacionadas(&conn, fuente, 12).expect("relacionadas");
        let pesos: Vec<u32> = relaciones.iter().map(|relacion| relacion.peso).collect();
        assert_eq!(pesos, vec![100, 40, 20, 20]);
        assert_eq!(relaciones[0].entrada.id, manual);
        assert_eq!(relaciones[1].entrada.id, jira);
        // Entre dos de mismo peso, la más reciente primero.
        assert_eq!(relaciones[2].entrada.id, app);
        assert_eq!(relaciones[3].entrada.id, app_vieja);
    }

    #[test]
    fn el_limite_recorta_y_contar_no() {
        let tmp = tempfile::tempdir().expect("dir temporal");
        let conn = bd(tmp.path());
        let fuente = texto(&conn, "PROJ-1896", 4);
        for _ in 0..3 {
            enlace(&conn, "https://empresa.atlassian.net/browse/PROJ-1896", 5);
        }
        // Tres capturas del mismo día se fusionan en una, así que la relación es una.
        assert_eq!(contar(&conn, fuente).expect("cuenta"), 1);
        assert_eq!(relacionadas(&conn, fuente, 0).expect("cero").len(), 0);
        assert_eq!(relacionadas(&conn, fuente, 1).expect("uno").len(), 1);
    }
}
