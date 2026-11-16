//! Tipos compartidos con el frontend (contrato, del líder). Espejo exacto en `src/types.ts`.
//! No cambiar nombres ni formas sin cambiar también `types.ts` y avisar al líder.

use serde::{Deserialize, Serialize};

// ───────────────────────── Enumeraciones ─────────────────────────

/// De dónde viene visualmente una entrada (color e icono en la UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fuente {
    Jira,
    Gitlab,
    Fs,
    Web,
    Texto,
    Nota,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TipoEntrada {
    Enlace,
    Texto,
    Nota,
}

impl TipoEntrada {
    pub fn como_str(self) -> &'static str {
        match self {
            Self::Enlace => "enlace",
            Self::Texto => "texto",
            Self::Nota => "nota",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origen {
    Navegador,
    Global,
    App,
    Cli,
}

impl Origen {
    pub fn como_str(self) -> &'static str {
        match self {
            Self::Navegador => "navegador",
            Self::Global => "global",
            Self::App => "app",
            Self::Cli => "cli",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TipoRecurso {
    Jira,
    Mr,
    Issue,
    Fs,
}

/// Qué se lista en la columna central.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vista {
    /// Un día concreto (`Filtro.dia`, por defecto hoy).
    Dia,
    /// Entradas cuyo recurso tiene cambios sin ver (cualquier día).
    Cambios,
    /// Entradas fijadas (cualquier día).
    Fijadas,
    /// Todo (búsqueda global), agrupado por día en la UI.
    Todo,
    /// Borradas en los últimos 30 días.
    Papelera,
}

// ───────────────────────── Recurso seguido ─────────────────────────

/// Recurso reconocido en una URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurso {
    pub tipo: TipoRecurso,
    /// `jira:PROJ-1896`, `mr:grupo/proy!39`, `issue:grupo/proy#12`, `fs:45600`.
    pub clave: String,
    /// URL canónica.
    pub url: String,
    /// Ruta GitLab (`grupo/proy`) o prefijo Jira (`PROJ`). `None` en FS.
    pub proyecto: Option<String>,
    /// Número/clave visible: `PROJ-1896`, `!39`, `#12`, `WI 1234`, `FS#45600`.
    pub visible: String,
    /// `true` si es una issue de `equipo/despliegues`.
    pub es_wi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comentario {
    pub id: String,
    pub autor: String,
    pub mio: bool,
    /// RFC 3339.
    pub fecha: String,
    /// Una línea, ≤ 240 caracteres.
    pub extracto: String,
}

/// Foto de un recurso remoto en un momento dado.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Huella {
    pub titulo: Option<String>,
    pub estado: Option<String>,
    pub asignado: Option<String>,
    pub etiquetas: Vec<String>,
    /// Estado del último pipeline (solo MR).
    pub pipeline: Option<String>,
    /// MR fusionada.
    pub fusionada: bool,
    pub actualizado: Option<String>,
    pub comentarios_total: u32,
    /// Los 10 comentarios más recientes (no de sistema), del más nuevo al más viejo.
    pub ultimos: Vec<Comentario>,
    /// RFC 3339 de cuándo se tomó.
    pub tomada: String,
}

/// Lo que ha cambiado desde la última vez que se vio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tipo",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Cambio {
    Comentarios {
        n: u32,
        autores: Vec<String>,
        extracto: String,
    },
    Estado {
        antes: Option<String>,
        despues: Option<String>,
    },
    Asignado {
        antes: Option<String>,
        despues: Option<String>,
    },
    Titulo {
        antes: Option<String>,
        despues: Option<String>,
    },
    Pipeline {
        antes: Option<String>,
        despues: Option<String>,
    },
    Fusionada,
    Etiquetas {
        anadidas: Vec<String>,
        quitadas: Vec<String>,
    },
}

// ───────────────────────── Vistas para el frontend ─────────────────────────

/// Trozo de un fragmento de búsqueda: `m = true` → resaltado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trozo {
    pub t: String,
    pub m: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntradaVista {
    pub id: i64,
    /// `YYYY-MM-DD`.
    pub dia: String,
    /// `HH:MM` local.
    pub hora: String,
    /// RFC 3339.
    pub creada: String,
    pub tipo: TipoEntrada,
    pub origen: Origen,
    pub fuente: Fuente,
    pub url: Option<String>,
    pub titulo: String,
    /// Clave visible del recurso (`PROJ-1896`, `!39`, `WI 1234`, `FS#45600`).
    pub clave: Option<String>,
    /// Estado remoto actual (de `recurso.estado`).
    pub estado: Option<String>,
    pub app: Option<String>,
    /// Ruta GitLab o prefijo Jira.
    pub proyecto: Option<String>,
    /// Host de la URL (`gitlab.com`) para webs genéricas.
    pub host: Option<String>,
    /// Selección recortada a 280 caracteres.
    pub seleccion: Option<String>,
    pub notas: u32,
    /// Última nota (tipo `nota`), recortada a 140 caracteres.
    pub ultima_nota: Option<String>,
    /// Vacío si no hay cambios sin ver.
    pub cambios: Vec<Cambio>,
    pub fijada: bool,
    pub relacionadas: u32,
    pub capturas: u32,
    /// Solo en búsquedas: fragmento con coincidencias.
    pub fragmento: Option<Vec<Trozo>>,
    /// Solo en papelera.
    pub borrada: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nota {
    pub id: i64,
    pub entrada_id: i64,
    pub creada: String,
    pub editada: Option<String>,
    /// `nota` | `cita`.
    pub tipo: String,
    pub texto: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relacion {
    pub entrada: EntradaVista,
    /// 1 o 2 motivos legibles (`PROJ-1896`, `App informes-web`, `Enlazada a mano`).
    pub motivos: Vec<String>,
    pub peso: u32,
    pub manual: bool,
}

/// Otra captura de la misma URL/recurso en otro día.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aparicion {
    pub id: i64,
    pub dia: String,
    pub hora: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecursoVista {
    pub clave: String,
    pub tipo: TipoRecurso,
    pub url: String,
    pub titulo: Option<String>,
    pub estado: Option<String>,
    pub asignado: Option<String>,
    pub comprobado: Option<String>,
    pub error: Option<String>,
    pub visto: Option<String>,
    pub seguir: bool,
    pub cambios: Vec<Cambio>,
    /// Comentarios recientes de la huella actual (más nuevo primero).
    pub ultimos: Vec<Comentario>,
    /// Ids de `ultimos` que son nuevos desde la última vez que se vio.
    pub nuevos: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetalleEntrada {
    pub entrada: EntradaVista,
    /// Selección completa (sin recortar).
    pub seleccion: Option<String>,
    pub notas: Vec<Nota>,
    pub relacionadas: Vec<Relacion>,
    pub historial: Vec<Aparicion>,
    pub recurso: Option<RecursoVista>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiaResumen {
    pub dia: String,
    pub total: u32,
    pub cambios: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filtro {
    pub vista: Option<Vista>,
    /// `YYYY-MM-DD` (solo vista `dia`; por defecto hoy).
    pub dia: Option<String>,
    /// Texto de búsqueda (FTS). Con texto, la vista `dia` busca solo en ese día.
    pub texto: Option<String>,
    /// Vacío = todas.
    pub fuentes: Vec<Fuente>,
    /// Por defecto 300.
    pub limite: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listado {
    pub entradas: Vec<EntradaVista>,
    /// Total sin límite.
    pub total: u32,
}

/// Captura que llega de la extensión, la ventana rápida, la app o la CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NuevaEntrada {
    pub tipo: TipoEntrada,
    pub origen: Origen,
    pub url: Option<String>,
    pub titulo: Option<String>,
    pub seleccion: Option<String>,
    pub nota: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultadoCaptura {
    pub id: i64,
    pub duplicada: bool,
    pub titulo: String,
    pub fuente: Fuente,
    pub clave: Option<String>,
    /// Nº de cambios sin ver del recurso.
    pub cambios: u32,
    pub relacionadas: u32,
}

/// Contenido para la ventana de captura rápida.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturaRapida {
    pub primaria: Option<String>,
    pub portapapeles: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstadoSeguimiento {
    pub en_curso: bool,
    pub ultima: Option<String>,
    pub proxima: Option<String>,
    /// Recursos seguidos en la última vuelta.
    pub seguidos: u32,
    /// Errores resumidos de la última vuelta (`Jira respondió 401`).
    pub errores: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstadoFuente {
    pub fuente: Fuente,
    /// Hay credenciales.
    pub configurada: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Integraciones {
    pub host_registrado: bool,
    /// RFC 3339 del último mensaje recibido de la extensión.
    pub extension_vista: Option<String>,
    /// `activo` | `desactivado` | `sin permiso` | `sin teclado`.
    pub doble_ctrl: String,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstadoApp {
    pub version: String,
    pub seguimiento: EstadoSeguimiento,
    pub fuentes: Vec<EstadoFuente>,
    pub integraciones: Integraciones,
    /// Entradas con cambios sin ver.
    pub cambios: u32,
    pub ruta_bd: String,
    pub ia_disponible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumenDia {
    pub dia: String,
    pub texto: String,
    pub generado: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ajustes {
    pub jira_base_url: String,
    pub gitlab_base_url: String,
    pub fs_base_url: String,
    pub prefijos_jira: Vec<String>,
    pub apps: Vec<String>,
    pub proyectos_dir: String,
    pub intervalo_min: u32,
    pub dias_seguimiento: u32,
    pub notificar: bool,
    pub doble_ctrl: bool,
    pub doble_ctrl_ms: u32,
    /// `sistema` | `oscuro` | `claro`.
    pub tema: String,
    pub ia: bool,
    pub ia_modelo: String,
    pub ia_base_url: String,
    /// Repositorio GitLab que hace de cola de work items (WI) de todas las apps, p. ej.
    /// `despliegues`. Vacío = sin WI: no se reconocen `WI 123` ni sus URLs.
    pub repo_wi: String,
}

impl Default for Ajustes {
    fn default() -> Self {
        Self {
            jira_base_url: "https://empresa.atlassian.net".into(),
            gitlab_base_url: "https://gitlab.com".into(),
            fs_base_url: "https://empresa.freshservice.com".into(),
            // En las pruebas, `PROJ` como si ya se hubiera leído una issue de ese proyecto.
            prefijos_jira: if cfg!(test) {
                vec!["PROJ".into()]
            } else {
                vec![]
            },
            apps: vec![],
            proyectos_dir: "~/Proyectos".into(),
            intervalo_min: 5,
            dias_seguimiento: 21,
            notificar: true,
            doble_ctrl: true,
            doble_ctrl_ms: 330,
            tema: "sistema".into(),
            ia: false,
            ia_modelo: "deepseek-v4.1-flash:1017".into(),
            ia_base_url: "https://ollama.com/v1".into(),
            repo_wi: String::new(),
        }
    }
}
