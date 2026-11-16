//! Diff de huellas: qué ha cambiado desde la última vez que se vio un recurso.
//! Todo son funciones puras sobre `model::Huella`.

use crate::model::{Cambio, Comentario, Huella};
use crate::util::parse_fecha;
use chrono::{DateTime, FixedOffset};

/// Cambios entre la huella vista y la actual, en el orden del contrato:
/// Comentarios, Estado, Fusionada, Pipeline, Asignado, Titulo, Etiquetas.
pub fn cambios(vista: &Huella, actual: &Huella) -> Vec<Cambio> {
    let mut salida = Vec::new();

    let nuevos = comentarios_nuevos(vista, actual);
    let ajenos: Vec<&Comentario> = nuevos.iter().copied().filter(|c| !c.mio).collect();
    let mios: u32 = u32::try_from(nuevos.len() - ajenos.len()).unwrap_or(0);
    let por_total = actual
        .comentarios_total
        .saturating_sub(vista.comentarios_total)
        .saturating_sub(mios);
    let n = u32::try_from(ajenos.len()).unwrap_or(0).max(por_total);
    if n > 0 {
        let mut autores: Vec<String> = Vec::new();
        for comentario in &ajenos {
            if !autores.contains(&comentario.autor) {
                autores.push(comentario.autor.clone());
            }
        }
        let extracto = ajenos
            .first()
            .map(|comentario| comentario.extracto.clone())
            .or_else(|| {
                actual
                    .ultimos
                    .first()
                    .map(|comentario| comentario.extracto.clone())
            })
            .unwrap_or_default();
        salida.push(Cambio::Comentarios {
            n,
            autores,
            extracto,
        });
    }

    if vista.estado != actual.estado {
        salida.push(Cambio::Estado {
            antes: vista.estado.clone(),
            despues: actual.estado.clone(),
        });
    }
    if actual.fusionada && !vista.fusionada {
        salida.push(Cambio::Fusionada);
    }
    if vista.pipeline != actual.pipeline {
        salida.push(Cambio::Pipeline {
            antes: vista.pipeline.clone(),
            despues: actual.pipeline.clone(),
        });
    }
    if vista.asignado != actual.asignado {
        salida.push(Cambio::Asignado {
            antes: vista.asignado.clone(),
            despues: actual.asignado.clone(),
        });
    }
    if vista.titulo != actual.titulo {
        salida.push(Cambio::Titulo {
            antes: vista.titulo.clone(),
            despues: actual.titulo.clone(),
        });
    }

    let anadidas: Vec<String> = actual
        .etiquetas
        .iter()
        .filter(|etiqueta| !vista.etiquetas.contains(etiqueta))
        .cloned()
        .collect();
    let quitadas: Vec<String> = vista
        .etiquetas
        .iter()
        .filter(|etiqueta| !actual.etiquetas.contains(etiqueta))
        .cloned()
        .collect();
    if !anadidas.is_empty() || !quitadas.is_empty() {
        salida.push(Cambio::Etiquetas { anadidas, quitadas });
    }

    salida
}

/// Ids de los comentarios nuevos ajenos, del más reciente al más viejo.
pub fn nuevos(vista: &Huella, actual: &Huella) -> Vec<String> {
    comentarios_nuevos(vista, actual)
        .into_iter()
        .filter(|c| !c.mio)
        .map(|c| c.id.clone())
        .collect()
}

/// Hay comentarios nuevos y todos son míos, y ningún otro cambio: la huella vista debe
/// avanzar aunque no haya nada que contar.
pub fn solo_mios(vista: &Huella, actual: &Huella) -> bool {
    let nuevos = comentarios_nuevos(vista, actual);
    !nuevos.is_empty() && nuevos.iter().all(|c| c.mio) && cambios(vista, actual).is_empty()
}

/// Comentarios de `actual` posteriores al más reciente de `vista` (todos si la vista no
/// tiene ventana). Solo se miran los 10 de la ventana de cada huella.
fn comentarios_nuevos<'a>(vista: &Huella, actual: &'a Huella) -> Vec<&'a Comentario> {
    let corte = corte(vista);
    actual
        .ultimos
        .iter()
        .filter(|comentario| match (parse_fecha(&comentario.fecha), corte) {
            (Some(fecha), Some(limite)) => fecha > limite,
            (Some(_), None) => true,
            (None, _) => false,
        })
        .collect()
}

fn corte(vista: &Huella) -> Option<DateTime<FixedOffset>> {
    vista
        .ultimos
        .iter()
        .filter_map(|comentario| parse_fecha(&comentario.fecha))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comentario(id: &str, autor: &str, mio: bool, fecha: &str) -> Comentario {
        Comentario {
            id: id.into(),
            autor: autor.into(),
            mio,
            fecha: fecha.into(),
            extracto: format!("texto {id}"),
        }
    }

    fn huella(ultimos: Vec<Comentario>, total: u32) -> Huella {
        Huella {
            comentarios_total: total,
            ultimos,
            ..Huella::default()
        }
    }

    fn fecha(dia: u32) -> String {
        format!("2026-11-{dia:02}T10:00:00+01:00")
    }

    #[test]
    fn vista_vacia_lo_cuenta_todo() {
        let vista = Huella::default();
        let actual = huella(
            vec![
                comentario("2", "Ana", false, &fecha(3)),
                comentario("1", "Ana", false, &fecha(2)),
            ],
            2,
        );
        assert_eq!(
            cambios(&vista, &actual),
            vec![Cambio::Comentarios {
                n: 2,
                autores: vec!["Ana".into()],
                extracto: "texto 2".into()
            }]
        );
        assert_eq!(
            nuevos(&vista, &actual),
            vec!["2".to_string(), "1".to_string()]
        );
    }

    #[test]
    fn comentarios_nuevos_ajenos_y_no_mios() {
        let vista = huella(vec![comentario("1", "Ana", false, &fecha(1))], 1);
        let actual = huella(
            vec![
                comentario("4", "Luis", false, &fecha(4)),
                comentario("3", "Yo", true, &fecha(3)),
                comentario("2", "Ana", false, &fecha(2)),
                comentario("1", "Ana", false, &fecha(1)),
            ],
            4,
        );
        assert_eq!(
            cambios(&vista, &actual),
            vec![Cambio::Comentarios {
                n: 2,
                autores: vec!["Luis".into(), "Ana".into()],
                extracto: "texto 4".into()
            }]
        );
        assert_eq!(
            nuevos(&vista, &actual),
            vec!["4".to_string(), "2".to_string()]
        );
    }

    #[test]
    fn comentarios_viejos_que_salen_de_la_ventana_no_cuentan() {
        // La vista conserva 10 comentarios viejos; en la actual ya no están (ventana de 10).
        let vista_ultimos: Vec<Comentario> = (1..=10)
            .map(|dia| comentario(&format!("v{dia}"), "Ana", false, &fecha(dia)))
            .collect();
        let vista = huella(vista_ultimos, 10);
        let actual_ultimos: Vec<Comentario> = (11..=20)
            .map(|dia| comentario(&format!("n{dia}"), "Ana", false, &fecha(dia)))
            .collect();
        let actual = huella(actual_ultimos, 20);

        let salida = cambios(&vista, &actual);
        assert_eq!(salida.len(), 1);
        let Cambio::Comentarios { n, autores, .. } = &salida[0] else {
            panic!("debería ser de comentarios: {salida:?}");
        };
        assert_eq!(*n, 10);
        assert_eq!(autores.len(), 1);
    }

    #[test]
    fn un_comentario_viejo_fuera_de_la_ventana_de_la_vista_no_es_nuevo() {
        let vista = huella(vec![comentario("5", "Ana", false, &fecha(5))], 10);
        let actual = huella(
            vec![
                comentario("6", "Ana", false, &fecha(6)),
                comentario("4", "Ana", false, &fecha(4)),
            ],
            11,
        );
        let salida = cambios(&vista, &actual);
        let Cambio::Comentarios { n, .. } = &salida[0] else {
            panic!("debería ser de comentarios: {salida:?}");
        };
        assert_eq!(*n, 1);
    }

    #[test]
    fn solo_mios_sin_otro_cambio() {
        let vista = huella(vec![comentario("1", "Ana", false, &fecha(1))], 1);
        let actual = huella(
            vec![
                comentario("2", "Yo", true, &fecha(2)),
                comentario("1", "Ana", false, &fecha(1)),
            ],
            2,
        );
        assert!(solo_mios(&vista, &actual));
        assert!(cambios(&vista, &actual).is_empty());
        assert!(nuevos(&vista, &actual).is_empty());
    }

    #[test]
    fn solo_mios_falla_si_hay_otro_cambio_o_comentario_ajeno() {
        let vista = huella(vec![comentario("1", "Ana", false, &fecha(1))], 1);
        let mut actual = huella(vec![comentario("2", "Yo", true, &fecha(2))], 2);
        actual.estado = Some("LISTO".into());
        assert!(!solo_mios(&vista, &actual));

        let actual = huella(
            vec![
                comentario("3", "Ana", false, &fecha(3)),
                comentario("2", "Yo", true, &fecha(2)),
            ],
            3,
        );
        assert!(!solo_mios(&vista, &actual));

        // Sin comentarios nuevos tampoco es "solo míos".
        assert!(!solo_mios(&vista, &vista));
    }

    #[test]
    fn estado_asignado_titulo_y_pipeline() {
        let vista = Huella {
            estado: Some("En curso".into()),
            asignado: Some("Ana".into()),
            titulo: Some("Viejo".into()),
            pipeline: None,
            ..Huella::default()
        };
        let actual = Huella {
            estado: Some("LISTO".into()),
            asignado: None,
            titulo: Some("Nuevo".into()),
            pipeline: Some("running".into()),
            ..Huella::default()
        };
        assert_eq!(
            cambios(&vista, &actual),
            vec![
                Cambio::Estado {
                    antes: Some("En curso".into()),
                    despues: Some("LISTO".into())
                },
                Cambio::Pipeline {
                    antes: None,
                    despues: Some("running".into())
                },
                Cambio::Asignado {
                    antes: Some("Ana".into()),
                    despues: None
                },
                Cambio::Titulo {
                    antes: Some("Viejo".into()),
                    despues: Some("Nuevo".into())
                },
            ]
        );
    }

    #[test]
    fn fusionada_solo_al_pasar_a_fusionada() {
        let vista = Huella::default();
        let actual = Huella {
            fusionada: true,
            ..Huella::default()
        };
        assert_eq!(cambios(&vista, &actual), vec![Cambio::Fusionada]);
        assert!(cambios(&actual, &actual).is_empty());
        assert!(cambios(&actual, &vista).is_empty());
    }

    #[test]
    fn etiquetas_anadidas_y_quitadas() {
        let vista = Huella {
            etiquetas: vec!["a".into(), "b".into()],
            ..Huella::default()
        };
        let actual = Huella {
            etiquetas: vec!["b".into(), "c".into()],
            ..Huella::default()
        };
        assert_eq!(
            cambios(&vista, &actual),
            vec![Cambio::Etiquetas {
                anadidas: vec!["c".into()],
                quitadas: vec!["a".into()]
            }]
        );
        assert!(cambios(&vista, &vista).is_empty());
    }

    #[test]
    fn el_cambio_de_actualizado_no_es_un_cambio() {
        let vista = Huella {
            actualizado: Some("2026-11-01T10:00:00+01:00".into()),
            ..Huella::default()
        };
        let actual = Huella {
            actualizado: Some("2026-11-04T10:00:00+01:00".into()),
            ..Huella::default()
        };
        assert!(cambios(&vista, &actual).is_empty());
        assert!(!solo_mios(&vista, &actual));
    }

    #[test]
    fn orden_completo_de_los_cambios() {
        let vista = Huella {
            estado: Some("A".into()),
            asignado: Some("Ana".into()),
            titulo: Some("Uno".into()),
            pipeline: Some("pending".into()),
            etiquetas: vec!["vieja".into()],
            ..Huella::default()
        };
        let actual = Huella {
            estado: Some("B".into()),
            asignado: Some("Luis".into()),
            titulo: Some("Dos".into()),
            pipeline: Some("success".into()),
            fusionada: true,
            etiquetas: vec!["nueva".into()],
            comentarios_total: 1,
            ultimos: vec![comentario("1", "Ana", false, &fecha(2))],
            ..Huella::default()
        };
        let tipos: Vec<&str> = cambios(&vista, &actual)
            .iter()
            .map(|cambio| match cambio {
                Cambio::Comentarios { .. } => "comentarios",
                Cambio::Estado { .. } => "estado",
                Cambio::Fusionada => "fusionada",
                Cambio::Pipeline { .. } => "pipeline",
                Cambio::Asignado { .. } => "asignado",
                Cambio::Titulo { .. } => "titulo",
                Cambio::Etiquetas { .. } => "etiquetas",
            })
            .collect();
        assert_eq!(
            tipos,
            vec![
                "comentarios",
                "estado",
                "fusionada",
                "pipeline",
                "asignado",
                "titulo",
                "etiquetas"
            ]
        );
    }
}
