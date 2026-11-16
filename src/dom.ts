import { createElement, icons } from 'lucide';
import logoUrl from './logo.svg';

export type Hijo = string | Node | null | undefined | false;
type Attrs = Record<string, unknown>;

export function esTauri(): boolean {
  return '__TAURI_INTERNALS__' in window;
}

/** Crea un nodo. Todo texto entra por `textContent`/nodos, nunca por HTML. */
export function h(tag: string, attrs: Attrs = {}, ...hijos: Hijo[]): HTMLElement {
  const el = document.createElement(tag);
  for (const [clave, valor] of Object.entries(attrs)) {
    if (valor === undefined || valor === null || valor === false) continue;
    if (clave === 'class') {
      el.className = String(valor);
    } else if (clave === 'text') {
      el.textContent = String(valor);
    } else if (clave.startsWith('on') && typeof valor === 'function') {
      el.addEventListener(clave.slice(2).toLowerCase(), valor as EventListener);
    } else if (clave === 'value' && 'value' in el) {
      (el as HTMLInputElement).value = String(valor);
    } else if (clave === 'checked' && 'checked' in el) {
      (el as HTMLInputElement).checked = Boolean(valor);
    } else if (typeof valor === 'boolean') {
      if (valor) el.setAttribute(clave, '');
    } else {
      el.setAttribute(clave, String(valor));
    }
  }
  for (const hijo of hijos) {
    if (hijo === null || hijo === undefined || hijo === false) continue;
    el.appendChild(typeof hijo === 'string' ? document.createTextNode(hijo) : hijo);
  }
  return el;
}

export function logo(clase = 'marca-logo'): HTMLImageElement {
  return h('img', { class: clase, src: logoUrl, alt: '' }) as HTMLImageElement;
}

function pascal(nombre: string): string {
  const especiales: Record<string, string> = {
    'git-merge': 'GitMerge',
    'git-branch': 'GitBranch',
    'life-buoy': 'LifeBuoy',
    'square-kanban': 'SquareKanban',
    'circle-dot': 'CircleDot',
    'text-quote': 'TextQuote',
    'pen-line': 'PenLine',
    'bell-dot': 'BellDot',
    'settings-2': 'Settings2',
    'trash-2': 'Trash2',
    'link-2': 'Link2',
    'panel-right-close': 'PanelRightClose',
    'arrow-up-right': 'ArrowUpRight',
    'arrow-right-left': 'ArrowRightLeft',
    'corner-down-right': 'CornerDownRight',
    'sticky-note': 'StickyNote',
    'chevron-left': 'ChevronLeft',
    'chevron-right': 'ChevronRight',
    'minus': 'Minus',
    'square': 'Square',
    'x': 'X',
  };
  if (especiales[nombre]) return especiales[nombre];
  return nombre
    .split('-')
    .map((parte) => (parte ? parte[0].toUpperCase() + parte.slice(1) : ''))
    .join('');
}

export function icono(nombre: string, clase = ''): SVGElement {
  const set = icons as unknown as Record<string, unknown>;
  const nodo = set[pascal(nombre)] ?? set.Circle;
  const svg = createElement(nodo as never) as unknown as SVGElement;
  svg.setAttribute('class', `ico${clase ? ` ${clase}` : ''}`);
  svg.setAttribute('stroke-width', '1.8');
  svg.setAttribute('aria-hidden', 'true');
  return svg;
}

export async function minimizar(): Promise<void> {
  if (!esTauri()) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow().minimize();
}

export async function alternarMaximizar(): Promise<void> {
  if (!esTauri()) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow().toggleMaximize();
}

export async function arrastrar(evento: MouseEvent): Promise<void> {
  if (!esTauri() || evento.button !== 0) return;
  const objetivo = evento.target as HTMLElement | null;
  if (objetivo?.closest('button, input, textarea, a, select')) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow().startDragging();
}

export function debounce(fn: () => void, ms: number): () => void {
  let temporizador: number | null = null;
  return () => {
    if (temporizador !== null) window.clearTimeout(temporizador);
    temporizador = window.setTimeout(() => {
      temporizador = null;
      fn();
    }, ms);
  };
}
