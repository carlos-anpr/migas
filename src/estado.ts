import type {
  Ajustes,
  DetalleEntrada,
  DiaResumen,
  EntradaVista,
  EstadoApp,
  EstadoSeguimiento,
  Filtro,
  Fuente,
  ResumenDia,
  Vista,
} from './types';
import { ipc } from './ipc';
import { hoy, pluralMigas, sumarDias } from './formato';
import { mostrarAviso } from './ui/avisos';

export type Servicio = typeof ipc;

/** Backend activo; el mock de desarrollo lo sustituye con `ponerMock`. */
export let api: Servicio = ipc;

export function ponerMock(servicio: Servicio): void {
  api = servicio;
}

export function ajustesDefecto(): Ajustes {
  return {
    jiraBaseUrl: 'https://empresa.atlassian.net',
    gitlabBaseUrl: 'https://gitlab.com',
    fsBaseUrl: 'https://empresa.freshservice.com',
    prefijosJira: ['PROJ'],
    apps: [],
    proyectosDir: '~/Proyectos',
    intervaloMin: 5,
    diasSeguimiento: 21,
    notificar: true,
    dobleCtrl: true,
    dobleCtrlMs: 330,
    tema: 'sistema',
    ia: false,
    iaModelo: 'deepseek-v4.1-flash:1017',
    iaBaseUrl: 'https://ollama.com/v1',
    repoWi: '',
  };
}

export interface EstadoUI {
  vista: Vista;
  dia: string;
  mesCalendario: string;
  texto: string;
  fuentes: Fuente[];
  seleccion: number | null;
  detalleAbierto: boolean;
  ajustes: Ajustes;
  estadoApp: EstadoApp | null;
  entradas: EntradaVista[];
  total: number;
  dias: DiaResumen[];
  fuentesConteo: Record<Fuente, number>;
  fijadasTotal: number;
  detalle: DetalleEntrada | null;
  cargando: boolean;
  seguimiento: EstadoSeguimiento | null;
  resumen: ResumenDia | null;
  resumiendo: boolean;
}

const conteoVacio = (): Record<Fuente, number> => ({
  jira: 0,
  gitlab: 0,
  fs: 0,
  web: 0,
  texto: 0,
  nota: 0,
});

let estado: EstadoUI = {
  vista: 'dia',
  dia: hoy(),
  mesCalendario: hoy().slice(0, 7),
  texto: '',
  fuentes: [],
  seleccion: null,
  detalleAbierto: true,
  ajustes: ajustesDefecto(),
  estadoApp: null,
  entradas: [],
  total: 0,
  dias: [],
  fuentesConteo: conteoVacio(),
  fijadasTotal: 0,
  detalle: null,
  cargando: false,
  seguimiento: null,
  resumen: null,
  resumiendo: false,
};

type Oyente = () => void;
const oyentes = new Set<Oyente>();
const borradoresNotas = new Map<number, string>();
let generacionListado = 0;
let generacionDias = 0;
let generacionDetalle = 0;
let generacionResumen = 0;

export function get(): EstadoUI {
  return estado;
}

export function set(parcial: Partial<EstadoUI>): void {
  estado = { ...estado, ...parcial };
  for (const oyente of oyentes) oyente();
}

export function suscribir(oyente: Oyente): () => void {
  oyentes.add(oyente);
  return () => {
    oyentes.delete(oyente);
  };
}

export function borradorDeNota(id: number): string {
  return borradoresNotas.get(id) ?? '';
}

export function guardarBorradorDeNota(id: number, texto: string): void {
  if (texto.length > 0) borradoresNotas.set(id, texto);
  else borradoresNotas.delete(id);
}

export function eliminarBorradorDeNota(id: number): void {
  borradoresNotas.delete(id);
}

export function filtroActual(): Filtro {
  return {
    vista: estado.vista,
    dia: estado.vista === 'dia' ? estado.dia : null,
    texto: estado.texto.trim() || null,
    fuentes: estado.fuentes,
    limite: 300,
  };
}

export async function recargar(): Promise<void> {
  const filtro = filtroActual();
  const diaResumen = filtro.dia;
  const genListado = ++generacionListado;
  const genDias = ++generacionDias;
  const genResumen = generacionResumen;
  try {
    const base: Filtro = { ...filtro, fuentes: [], limite: 1 };
    const fuentes: Fuente[] = ['jira', 'gitlab', 'fs', 'web', 'texto'];
    const [listado, dias, app, fijadas, ...conteos] = await Promise.all([
      api.listar(filtro),
      api.listarDias(null, null),
      api.estadoApp(),
      api.listar({ ...filtro, vista: 'fijadas', limite: 1 }),
      ...fuentes.map((fuente) => api.listar({ ...base, fuentes: [fuente] })),
    ]);
    const listadoVigente = genListado === generacionListado;
    const diasVigente = genDias === generacionDias;
    if (!listadoVigente && !diasVigente) return;
    if (diasVigente) set({ dias });
    if (!listadoVigente) return;

    const fuentesConteo = conteoVacio();
    fuentes.forEach((fuente, indice) => {
      fuentesConteo[fuente] = conteos[indice]?.total ?? 0;
    });
    const ids = new Set(listado.entradas.map((entrada) => entrada.id));
    let seleccion = estado.seleccion;
    if (seleccion === null || !ids.has(seleccion)) {
      seleccion = listado.entradas[0]?.id ?? null;
    }
    const detalleActual =
      estado.detalle?.entrada.id === seleccion ? estado.detalle : null;
    set({
      entradas: listado.entradas,
      total: listado.total,
      estadoApp: app,
      fijadasTotal: fijadas.total,
      fuentesConteo,
      seleccion,
      detalle: estado.detalleAbierto ? detalleActual : null,
      cargando: false,
      seguimiento: app.seguimiento,
    });
    if (seleccion !== null && estado.detalleAbierto) {
      await cargarDetalle(seleccion);
    } else if (seleccion === null) {
      set({ detalle: null });
    }
    if (
      genListado !== generacionListado ||
      genResumen !== generacionResumen
    ) {
      return;
    }
    if (estado.vista === 'dia' && diaResumen && !estado.resumiendo) {
      const resumen = await api.resumenDia(diaResumen).catch(() => null);
      if (
        genListado === generacionListado &&
        genResumen === generacionResumen &&
        estado.vista === 'dia' &&
        estado.dia === diaResumen
      ) {
        set({ resumen });
      }
    } else if (estado.vista !== 'dia') {
      set({ resumen: null });
    }
  } catch (error) {
    if (genListado !== generacionListado) return;
    set({ cargando: false });
    mostrarAviso(`No se pudo cargar: ${String(error)}`, { mal: true });
  }
}

export async function cargarDetalle(id: number | null): Promise<void> {
  if (id === null) {
    generacionDetalle += 1;
    set({ detalle: null });
    return;
  }
  const generacion = ++generacionDetalle;
  try {
    const detalle = await api.detalle(id);
    if (generacion !== generacionDetalle || estado.seleccion !== id) return;
    set({ detalle });
  } catch {
    if (generacion !== generacionDetalle || estado.seleccion !== id) return;
    set({ detalle: null });
  }
}

export async function seleccionar(id: number | null): Promise<void> {
  if (id === null) {
    set({ seleccion: null, detalle: null, detalleAbierto: false });
    return;
  }
  set({
    seleccion: id,
    detalleAbierto: true,
    detalle: estado.detalle?.entrada.id === id ? estado.detalle : null,
  });
  await cargarDetalle(id);
}

export async function irVista(vista: Vista): Promise<void> {
  generacionResumen += 1;
  set({
    vista,
    texto: '',
    mesCalendario: estado.dia.slice(0, 7),
    resumen: vista === 'dia' ? estado.resumen : null,
    resumiendo: false,
  });
  await recargar();
}

export async function irDia(dia: string): Promise<void> {
  generacionResumen += 1;
  set({
    vista: 'dia',
    dia,
    mesCalendario: dia.slice(0, 7),
    texto: '',
    resumen: null,
    resumiendo: false,
  });
  await recargar();
}

export async function irHoy(): Promise<void> {
  await irDia(hoy());
}

export async function saltarA(id: number, dia: string): Promise<void> {
  if (estado.vista !== 'dia' || estado.dia !== dia) {
    generacionResumen += 1;
    set({
      vista: 'dia',
      dia,
      mesCalendario: dia.slice(0, 7),
      texto: '',
      resumen: null,
      resumiendo: false,
    });
    await recargar();
  }
  await seleccionar(id);
}

/** Deja la lista mostrando solo los resultados de `texto` y abre la miga elegida. */
export async function mostrarBusqueda(texto: string, id: number): Promise<void> {
  generacionResumen += 1;
  set({ vista: 'todo', texto, resumen: null, resumiendo: false });
  await recargar();
  await seleccionar(id);
}

export async function quitarBusqueda(): Promise<void> {
  set({ texto: '' });
  await recargar();
}

export async function comprobarAhora(): Promise<void> {
  try {
    await api.comprobarAhora();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function ocultarPrincipal(): Promise<void> {
  try {
    await api.ocultarPrincipal();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function generarResumenDia(): Promise<void> {
  if (estado.resumiendo || estado.vista !== 'dia' || !estado.estadoApp?.iaDisponible) {
    return;
  }
  const dia = estado.dia;
  const generacion = ++generacionResumen;
  set({ resumiendo: true });
  try {
    const resumen = await api.generarResumen(dia);
    if (generacion === generacionResumen) {
      if (estado.vista === 'dia' && estado.dia === dia) {
        set({ resumen, resumiendo: false });
      } else {
        set({ resumiendo: false });
      }
    }
  } catch (error) {
    if (generacion === generacionResumen) set({ resumiendo: false });
    mostrarAviso(String(error), { mal: true });
  }
}

export async function buscar(texto: string): Promise<void> {
  set({ texto });
  await recargar();
}

export async function alternarFuente(fuente: Fuente): Promise<void> {
  const fuentes = estado.fuentes.includes(fuente)
    ? estado.fuentes.filter((actual) => actual !== fuente)
    : [...estado.fuentes, fuente];
  set({ fuentes });
  await recargar();
}

export async function limpiarFuentes(): Promise<void> {
  set({ fuentes: [] });
  await recargar();
}

export function esDiaConEntradas(dia: string): boolean {
  return estado.dias.some((resumen) => resumen.dia === dia);
}

export function diasConEntradas(): string[] {
  return estado.dias.map((resumen) => resumen.dia);
}

export function diaAnteriorConEntradas(): string | null {
  const dias = diasConEntradas();
  const indice = dias.indexOf(estado.dia);
  if (indice >= 0 && indice + 1 < dias.length) return dias[indice + 1];
  return dias.find((dia) => dia < estado.dia) ?? null;
}

export function diaSiguienteConEntradas(): string | null {
  const dias = diasConEntradas();
  const indice = dias.indexOf(estado.dia);
  if (indice > 0) return dias[indice - 1];
  return dias.find((dia) => dia > estado.dia) ?? null;
}

export async function fijar(id: number, fijada: boolean): Promise<void> {
  const anterior = estado.entradas.find((entrada) => entrada.id === id)?.fijada ?? false;
  set({
    entradas: estado.entradas.map((entrada) =>
      entrada.id === id ? { ...entrada, fijada } : entrada,
    ),
  });
  try {
    await api.fijar(id, fijada);
    await recargar();
  } catch (error) {
    set({
      entradas: estado.entradas.map((entrada) =>
        entrada.id === id ? { ...entrada, fijada: anterior } : entrada,
      ),
    });
    mostrarAviso(String(error), { mal: true });
  }
}

export async function borrar(id: number): Promise<void> {
  const entrada = estado.entradas.find((actual) => actual.id === id);
  if (!entrada) return;
  const indice = estado.entradas.findIndex((actual) => actual.id === id);
  set({
    entradas: estado.entradas.filter((actual) => actual.id !== id),
    seleccion: estado.seleccion === id ? null : estado.seleccion,
    detalle: estado.seleccion === id ? null : estado.detalle,
  });
  try {
    await api.borrar(id);
    mostrarAviso('Movida a la papelera', {
      accion: {
        texto: 'Deshacer',
        fn: async () => {
          try {
            await api.restaurar(id);
            await recargar();
          } catch (error) {
            mostrarAviso(String(error), { mal: true });
          }
        },
      },
      ms: 6000,
    });
    await recargar();
  } catch (error) {
    const actuales = [...estado.entradas];
    actuales.splice(Math.max(0, indice), 0, entrada);
    set({ entradas: actuales, seleccion: id });
    mostrarAviso(String(error), { mal: true });
  }
}

/** Pide confirmación y, solo si se pulsa «Borrar», elimina la miga para siempre. */
export function borrarDefinitivo(id: number): void {
  const entrada =
    estado.entradas.find((actual) => actual.id === id) ??
    (estado.detalle?.entrada.id === id ? estado.detalle.entrada : undefined);
  const sello = entrada?.borrada;
  if (!sello) return;
  const fn = async (): Promise<void> => {
    try {
      await api.borrarDefinitivo(id, sello);
      set({
        entradas: estado.entradas.filter((actual) => actual.id !== id),
        seleccion: estado.seleccion === id ? null : estado.seleccion,
        detalle: estado.detalle?.entrada.id === id ? null : estado.detalle,
      });
      await recargar();
      mostrarAviso('Borrada definitivamente');
    } catch (error) {
      mostrarAviso(String(error), { mal: true });
    }
  };
  mostrarAviso('¿Borrar para siempre? No se puede deshacer', {
    accion: { texto: 'Borrar', fn },
    ms: 6000,
  });
}

/** Pide confirmación y, solo si se pulsa «Vaciar», borra la papelera entera. */
export function vaciarPapelera(): void {
  const total = estado.total;
  const fn = async (): Promise<void> => {
    try {
      const borradas = await api.vaciarPapelera();
      // Si el usuario ya salió de la papelera, su vista no pierde nada: solo se recarga.
      if (estado.vista === 'papelera') {
        set({ entradas: [], total: 0, seleccion: null, detalle: null });
      }
      await recargar();
      mostrarAviso(`${pluralMigas(borradas)} ${borradas === 1 ? 'borrada' : 'borradas'}`);
    } catch (error) {
      mostrarAviso(String(error), { mal: true });
    }
  };
  // Con filtro o búsqueda, `total` solo cuenta las visibles y se vacía la papelera entera.
  const filtrada = estado.texto.trim() !== '' || estado.fuentes.length > 0;
  const cuantas = filtrada
    ? 'Se borran todas las migas de la papelera, no solo las filtradas,'
    : `${total === 1 ? 'Se borra' : 'Se borran'} ${pluralMigas(total)}`;
  mostrarAviso(`¿Vaciar la papelera? ${cuantas} para siempre`, {
    accion: { texto: 'Vaciar', fn },
    ms: 6000,
  });
}

export async function abrirEntrada(id: number): Promise<void> {
  try {
    await api.abrir(id);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function marcarVisto(id: number): Promise<void> {
  try {
    await api.marcarVisto(id);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function marcarTodoVisto(): Promise<void> {
  try {
    const marcadas = await api.marcarTodoVisto();
    mostrarAviso(`${marcadas} ${marcadas === 1 ? 'recurso marcado' : 'recursos marcados'}`);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function copiar(texto: string): Promise<void> {
  try {
    await api.copiar(texto);
    mostrarAviso('Copiado');
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

/** Copia el contenido completo: URL, detalle.seleccion o la nota completa. */
export async function copiarEntrada(entrada: EntradaVista): Promise<void> {
  if (entrada.url) {
    await copiar(entrada.url);
    return;
  }
  let detalle =
    estado.detalle?.entrada.id === entrada.id ? estado.detalle : null;
  if (!detalle) {
    try {
      detalle = await api.detalle(entrada.id);
    } catch {
      // Se usa lo que haya en la vista si no se puede abrir el detalle.
    }
  }
  let texto = entrada.titulo;
  if (detalle?.seleccion) {
    texto = detalle.seleccion;
  } else if (entrada.tipo === 'nota') {
    const notas = detalle?.notas ?? [];
    const nota = [...notas].reverse().find((actual) => actual.tipo === 'nota');
    texto = nota?.texto ?? entrada.ultimaNota ?? entrada.titulo;
  } else {
    texto = detalle?.seleccion ?? entrada.seleccion ?? entrada.titulo;
  }
  await copiar(texto);
}

export async function editarTitulo(id: number, titulo: string): Promise<void> {
  try {
    await api.editarTitulo(id, titulo);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function crearNota(
  entradaId: number,
  texto: string,
): Promise<boolean> {
  try {
    await api.crearNota(entradaId, texto);
    await recargar();
    return true;
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
    return false;
  }
}

export async function editarNota(id: number, texto: string): Promise<boolean> {
  try {
    await api.editarNota(id, texto);
    await recargar();
    return true;
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
    return false;
  }
}

export async function borrarNota(id: number): Promise<void> {
  try {
    await api.borrarNota(id);
    mostrarAviso('Nota borrada');
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function restaurarEntrada(id: number): Promise<void> {
  try {
    await api.restaurar(id);
    mostrarAviso('Restaurada');
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function enlazar(a: number, b: number): Promise<void> {
  try {
    await api.enlazar(a, b);
    mostrarAviso('Enlazadas');
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function desenlazar(a: number, b: number): Promise<void> {
  try {
    await api.desenlazar(a, b);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function descartar(a: number, b: number): Promise<void> {
  try {
    await api.descartar(a, b);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function seguir(id: number, seguir: boolean): Promise<void> {
  try {
    await api.seguir(id, seguir);
    await recargar();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export async function guardarAjustes(parcial: Partial<Ajustes>): Promise<void> {
  const ajustes = { ...estado.ajustes, ...parcial };
  set({ ajustes });
  try {
    await api.guardarAjustes(ajustes);
    mostrarAviso('Ajustes guardados');
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

export function entradaSeleccionada(): EntradaVista | null {
  return estado.entradas.find((entrada) => entrada.id === estado.seleccion) ?? null;
}

let temaActual: Ajustes['tema'] = 'sistema';
const medioClaro = window.matchMedia('(prefers-color-scheme: light)');
medioClaro.addEventListener('change', () => aplicarTema(temaActual));

export function aplicarTema(tema: Ajustes['tema']): void {
  temaActual = tema;
  const claro = tema === 'claro' || (tema === 'sistema' && medioClaro.matches);
  if (claro) document.documentElement.dataset.tema = 'claro';
  else delete document.documentElement.dataset.tema;
}

export function moverSeleccion(delta: number): void {
  if (estado.entradas.length === 0) return;
  const indice = estado.entradas.findIndex((entrada) => entrada.id === estado.seleccion);
  const siguiente = Math.min(
    Math.max((indice < 0 ? 0 : indice) + delta, 0),
    estado.entradas.length - 1,
  );
  const entrada = estado.entradas[siguiente];
  if (entrada) void seleccionar(entrada.id);
}

export { hoy, sumarDias };
