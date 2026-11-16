import './styles.css';
import { debounce, esTauri, h } from './dom';
import { eventos } from './ipc';
import {
  api,
  aplicarTema,
  get,
  ponerMock,
  recargar,
  saltarA,
  set,
  suscribir,
} from './estado';
import { instalarTeclas } from './teclas';
import { abrirAjustes } from './ui/ajustes';
import { pintarBarra } from './ui/barra';
import { pintarDetalle } from './ui/detalle';
import { pintarLinea } from './ui/linea';
import { abrirPaleta } from './ui/paleta';
import { pintarRail } from './ui/rail';
import { mostrarAviso } from './ui/avisos';

let ultimoScroll = 0;
let ultimoDetalleId: number | null = null;

function pintar(): void {
  const app = document.getElementById('app');
  if (!app) return;
  const listaAnterior = document.querySelector<HTMLElement>('.lista');
  if (listaAnterior) ultimoScroll = listaAnterior.scrollTop;
  const activo = document.activeElement;
  const idCompositor = get().seleccion;
  const compositor =
    activo instanceof HTMLTextAreaElement && activo.closest('.compositor')
      ? { valor: activo.value, inicio: activo.selectionStart, fin: activo.selectionEnd, id: idCompositor }
      : null;

  // Editor con `data-foco` (p. ej. una nota en edición): recupera foco y cursor tras repintar.
  const foco =
    (activo instanceof HTMLTextAreaElement || activo instanceof HTMLInputElement) && activo.dataset.foco
      ? { clave: activo.dataset.foco, inicio: activo.selectionStart, fin: activo.selectionEnd }
      : null;

  const estado = get();
  const detalleAnterior = document.querySelector<HTMLElement>('.detalle');
  const reutilizarDetalle =
    compositor !== null &&
    estado.detalleAbierto &&
    estado.detalle !== null &&
    estado.detalle.entrada.id === ultimoDetalleId &&
    estado.detalle.entrada.id === estado.seleccion &&
    detalleAnterior !== null;
  if (reutilizarDetalle) detalleAnterior.remove();

  const contenedor = h('div', { class: `app${estado.detalleAbierto ? '' : ' sin-detalle'}` });
  contenedor.appendChild(pintarBarra(() => abrirPaleta(), () => abrirAjustes()));
  contenedor.appendChild(pintarRail((seccion) => abrirAjustes(seccion)));
  contenedor.appendChild(pintarLinea());
  contenedor.appendChild(reutilizarDetalle ? detalleAnterior : pintarDetalle(estado));
  app.replaceChildren(contenedor);
  ultimoDetalleId = estado.detalle?.entrada.id ?? null;

  const lista = document.querySelector<HTMLElement>('.lista');
  if (lista) lista.scrollTop = ultimoScroll;
  if (compositor && get().seleccion === compositor.id) {
    const area = document.querySelector<HTMLTextAreaElement>('.compositor textarea');
    if (area) {
      area.value = compositor.valor;
      area.focus();
      area.setSelectionRange(compositor.inicio, compositor.fin);
    }
  }
  if (foco) {
    const campo = document.querySelector<HTMLTextAreaElement | HTMLInputElement>(
      `[data-foco="${CSS.escape(foco.clave)}"]`,
    );
    if (campo && document.activeElement !== campo) {
      campo.focus();
      campo.setSelectionRange(foco.inicio, foco.fin);
    }
  }
}

async function abrirRapidaNota(): Promise<void> {
  try {
    await api.abrirRapida();
  } catch (error) {
    mostrarAviso(String(error), { mal: true });
  }
}

async function iniciar(): Promise<void> {
  const parametros = new URLSearchParams(location.search);
  if (parametros.has('mock')) {
    const { crearMock } = await import('./mock');
    ponerMock(crearMock());
  }
  try {
    const ajustes = await api.obtenerAjustes();
    set({ ajustes });
    aplicarTema(ajustes.tema);
  } catch (error) {
    mostrarAviso(`No se pudieron cargar los ajustes: ${String(error)}`, { mal: true });
  }
  await recargar();
  try {
    const pendiente = await api.mostrarPendiente();
    if (pendiente !== null) {
      const detalle = await api.detalle(pendiente);
      await saltarA(pendiente, detalle.entrada.dia);
    }
  } catch {
    // El backend puede no tener todavía el comando; no es un error de la UI.
  }
  instalarTeclas({
    abrirPaleta: () => abrirPaleta(),
    abrirAjustes: () => abrirAjustes(),
    abrirRapida: () => void abrirRapidaNota(),
  });
  suscribir(() => pintar());
  pintar();

  if (esTauri()) {
    await eventos.datos(debounce(() => void recargar(), 150));
    await eventos.seguimiento((estadoSeguimiento) => set({ seguimiento: estadoSeguimiento }));
    await eventos.mostrar((id) => {
      if (id === null) {
        set({ detalleAbierto: true });
        return;
      }
      void api.detalle(id).then((detalle) => saltarA(id, detalle.entrada.dia));
    });
  }

  window.addEventListener('migas:ajustes', (evento) => {
    abrirAjustes((evento as CustomEvent<string>).detail);
  });
  window.addEventListener('migas:repintar', () => {
    ultimoDetalleId = null;
    pintar();
  });
  window.addEventListener('migas:paleta-enlazar', (evento) => {
    abrirPaleta((evento as CustomEvent<number>).detail);
  });

  if (location.hash === '#paleta') {
    abrirPaleta(undefined, parametros.has('mock') ? 'cupones' : '');
  } else if (location.hash === '#ajustes') {
    abrirAjustes();
  } else if (location.hash === '#claro') {
    aplicarTema('claro');
  }
}

void iniciar();
