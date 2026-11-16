// Contrato con el backend (del líder). Espejo exacto de src-tauri/src/model.rs.

export type Fuente = "jira" | "gitlab" | "fs" | "web" | "texto" | "nota";
export type TipoEntrada = "enlace" | "texto" | "nota";
export type Origen = "navegador" | "global" | "app" | "cli";
export type TipoRecurso = "jira" | "mr" | "issue" | "fs";
export type Vista = "dia" | "cambios" | "fijadas" | "todo" | "papelera";

export interface Comentario {
  id: string;
  autor: string;
  mio: boolean;
  fecha: string;
  extracto: string;
}

export type Cambio =
  | { tipo: "comentarios"; n: number; autores: string[]; extracto: string }
  | { tipo: "estado"; antes: string | null; despues: string | null }
  | { tipo: "asignado"; antes: string | null; despues: string | null }
  | { tipo: "titulo"; antes: string | null; despues: string | null }
  | { tipo: "pipeline"; antes: string | null; despues: string | null }
  | { tipo: "fusionada" }
  | { tipo: "etiquetas"; anadidas: string[]; quitadas: string[] };

export interface Trozo {
  t: string;
  m: boolean;
}

export interface EntradaVista {
  id: number;
  dia: string;
  hora: string;
  creada: string;
  tipo: TipoEntrada;
  origen: Origen;
  fuente: Fuente;
  url: string | null;
  titulo: string;
  clave: string | null;
  estado: string | null;
  app: string | null;
  proyecto: string | null;
  host: string | null;
  seleccion: string | null;
  notas: number;
  ultimaNota: string | null;
  cambios: Cambio[];
  fijada: boolean;
  relacionadas: number;
  capturas: number;
  fragmento: Trozo[] | null;
  borrada: string | null;
}

export interface Nota {
  id: number;
  entradaId: number;
  creada: string;
  editada: string | null;
  tipo: "nota" | "cita";
  texto: string;
}

export interface Relacion {
  entrada: EntradaVista;
  motivos: string[];
  peso: number;
  manual: boolean;
}

export interface Aparicion {
  id: number;
  dia: string;
  hora: string;
}

export interface RecursoVista {
  clave: string;
  tipo: TipoRecurso;
  url: string;
  titulo: string | null;
  estado: string | null;
  asignado: string | null;
  comprobado: string | null;
  error: string | null;
  visto: string | null;
  seguir: boolean;
  cambios: Cambio[];
  ultimos: Comentario[];
  nuevos: string[];
}

export interface DetalleEntrada {
  entrada: EntradaVista;
  seleccion: string | null;
  notas: Nota[];
  relacionadas: Relacion[];
  historial: Aparicion[];
  recurso: RecursoVista | null;
}

export interface DiaResumen {
  dia: string;
  total: number;
  cambios: number;
}

export interface Filtro {
  vista?: Vista | null;
  dia?: string | null;
  texto?: string | null;
  fuentes?: Fuente[];
  limite?: number | null;
}

export interface Listado {
  entradas: EntradaVista[];
  total: number;
}

export interface NuevaEntrada {
  tipo: TipoEntrada;
  origen: Origen;
  url: string | null;
  titulo: string | null;
  seleccion: string | null;
  nota: string | null;
}

export interface ResultadoCaptura {
  id: number;
  duplicada: boolean;
  titulo: string;
  fuente: Fuente;
  clave: string | null;
  cambios: number;
  relacionadas: number;
}

export interface CapturaRapida {
  primaria: string | null;
  portapapeles: string | null;
}

export interface EstadoSeguimiento {
  enCurso: boolean;
  ultima: string | null;
  proxima: string | null;
  seguidos: number;
  errores: string[];
}

export interface EstadoFuente {
  fuente: Fuente;
  configurada: boolean;
  error: string | null;
}

export interface Integraciones {
  hostRegistrado: boolean;
  extensionVista: string | null;
  dobleCtrl: "activo" | "desactivado" | "sin permiso" | "sin teclado";
  autostart: boolean;
}

export interface EstadoApp {
  version: string;
  seguimiento: EstadoSeguimiento;
  fuentes: EstadoFuente[];
  integraciones: Integraciones;
  cambios: number;
  rutaBd: string;
  iaDisponible: boolean;
}

export interface ResumenDia {
  dia: string;
  texto: string;
  generado: string;
}

export interface Ajustes {
  jiraBaseUrl: string;
  gitlabBaseUrl: string;
  fsBaseUrl: string;
  prefijosJira: string[];
  apps: string[];
  proyectosDir: string;
  intervaloMin: number;
  diasSeguimiento: number;
  notificar: boolean;
  dobleCtrl: boolean;
  dobleCtrlMs: number;
  tema: "sistema" | "oscuro" | "claro";
  ia: boolean;
  iaModelo: string;
  iaBaseUrl: string;
  repoWi: string;
}
