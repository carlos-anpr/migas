// Contrato IPC (del líder). Cada función ⇄ un #[tauri::command] de lib.rs con el mismo
// nombre en snake_case. Los argumentos van en camelCase (Tauri los convierte).

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Ajustes,
  CapturaRapida,
  DetalleEntrada,
  DiaResumen,
  EstadoApp,
  EstadoSeguimiento,
  Filtro,
  Listado,
  Nota,
  NuevaEntrada,
  ResultadoCaptura,
  ResumenDia,
} from "./types";

export const ipc = {
  /** Días con entradas (desc). `desde`/`hasta` = YYYY-MM-DD inclusivos; null = todos. */
  listarDias: (desde: string | null, hasta: string | null) =>
    invoke<DiaResumen[]>("listar_dias", { desde, hasta }),
  listar: (filtro: Filtro) => invoke<Listado>("listar", { filtro }),
  detalle: (id: number) => invoke<DetalleEntrada>("detalle", { id }),
  /** Abre la URL en el navegador y marca visto el recurso. Error si no tiene URL. */
  abrir: (id: number) => invoke<void>("abrir", { id }),
  marcarVisto: (id: number) => invoke<void>("marcar_visto", { id }),
  marcarTodoVisto: () => invoke<number>("marcar_todo_visto"),
  crearEntrada: (nueva: NuevaEntrada) => invoke<ResultadoCaptura>("crear_entrada", { nueva }),
  editarTitulo: (id: number, titulo: string) => invoke<void>("editar_titulo", { id, titulo }),
  fijar: (id: number, fijada: boolean) => invoke<void>("fijar", { id, fijada }),
  /** A la papelera (recuperable 30 días). */
  borrar: (id: number) => invoke<void>("borrar", { id }),
  restaurar: (id: number) => invoke<void>("restaurar", { id }),
  /** Borra para siempre una entrada que ya está en la papelera. */
  borrarDefinitivo: (id: number, sello: string) =>
    invoke<void>("borrar_definitivo", { id, sello }),
  /** Vacía la papelera entera. Devuelve cuántas migas borró. */
  vaciarPapelera: () => invoke<number>("vaciar_papelera"),
  crearNota: (entradaId: number, texto: string) => invoke<Nota>("crear_nota", { entradaId, texto }),
  editarNota: (id: number, texto: string) => invoke<Nota>("editar_nota", { id, texto }),
  borrarNota: (id: number) => invoke<void>("borrar_nota", { id }),
  enlazar: (a: number, b: number) => invoke<void>("enlazar", { a, b }),
  desenlazar: (a: number, b: number) => invoke<void>("desenlazar", { a, b }),
  /** Oculta una relación automática entre dos entradas. */
  descartar: (a: number, b: number) => invoke<void>("descartar", { a, b }),
  /** Deja de comprobar cambios del recurso de esta entrada (o vuelve a hacerlo). */
  seguir: (id: number, seguir: boolean) => invoke<void>("seguir", { id, seguir }),
  comprobarAhora: () => invoke<void>("comprobar_ahora"),
  estadoApp: () => invoke<EstadoApp>("estado_app"),
  obtenerAjustes: () => invoke<Ajustes>("obtener_ajustes"),
  guardarAjustes: (ajustes: Ajustes) => invoke<void>("guardar_ajustes", { ajustes }),
  /** Registra host nativo + lanzador + autostart. Devuelve un informe de una línea por paso. */
  instalarIntegraciones: () => invoke<string>("instalar_integraciones"),
  /** Ruta absoluta de la carpeta `extension/` para cargarla en Chrome. */
  rutaExtension: () => invoke<string>("ruta_extension"),
  copiar: (texto: string) => invoke<void>("copiar", { texto }),
  /** Abre la ventana rápida vacía (modo Nota), colocada y enfocada por el backend (Ctrl+N). */
  abrirRapida: () => invoke<void>("abrir_rapida"),
  ocultarPrincipal: () => invoke<void>("ocultar_principal"),
  /** Ventana rápida: contenido capturado pendiente. */
  capturaPendiente: () => invoke<CapturaRapida>("captura_pendiente"),
  cerrarRapida: () => invoke<void>("cerrar_rapida"),
  /** Abre la ventana principal (desde la rápida) con la entrada seleccionada. */
  mostrarPrincipal: (id: number | null) => invoke<void>("mostrar_principal", { id }),
  /** Id a seleccionar pedido con `--mostrar <id>` antes de que la UI escuchara; se consume (devuelve null la 2.ª vez). Llamar al arrancar main.ts. */
  mostrarPendiente: () => invoke<number | null>("mostrar_pendiente"),
  resumenDia: (dia: string) => invoke<ResumenDia | null>("resumen_dia", { dia }),
  generarResumen: (dia: string) => invoke<ResumenDia>("generar_resumen", { dia }),
  salir: () => invoke<void>("salir"),
};

export const eventos = {
  /** La BD ha cambiado (otra ventana, la extensión, el seguimiento). Recargar lo visible. */
  datos: (f: () => void): Promise<UnlistenFn> => listen("migas://datos", () => f()),
  seguimiento: (f: (e: EstadoSeguimiento) => void): Promise<UnlistenFn> =>
    listen<EstadoSeguimiento>("migas://seguimiento", (ev) => f(ev.payload)),
  /** Mostrar la principal y seleccionar `id` (si no es null). */
  mostrar: (f: (id: number | null) => void): Promise<UnlistenFn> =>
    listen<{ id: number | null }>("migas://mostrar", (ev) => f(ev.payload.id)),
  /** Ventana rápida: llega contenido nuevo (se reabrió con otra captura). */
  rapida: (f: (c: CapturaRapida) => void): Promise<UnlistenFn> =>
    listen<CapturaRapida>("migas://rapida", (ev) => f(ev.payload)),
};
