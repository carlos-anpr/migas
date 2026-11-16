import './styles.css';
import type { CapturaRapida, NuevaEntrada, ResultadoCaptura } from './types';
import { arrastrar, h, icono, logo } from './dom';
import { api, aplicarTema, get, ponerMock, set } from './estado';
import { detectarClaves, esUrlUnica } from './formato';
import { eventos } from './ipc';
import { mostrarAviso } from './ui/avisos';

type Modo = 'seleccion' | 'portapapeles' | 'nota';

const MODOS: Array<{ id: Modo; texto: string }> = [
  { id: 'seleccion', texto: 'Selección' },
  { id: 'portapapeles', texto: 'Portapapeles' },
  { id: 'nota', texto: 'Nota' },
];

let modo: Modo = 'nota';
const textos: Record<Modo, string> = { seleccion: '', portapapeles: '', nota: '' };
let notaTexto = '';
let cierreGuardado: number | null = null;
let generacionCaptura = 1;

const raiz = document.getElementById('rapida') ?? document.body;
const contenedor = h('div', { class: 'rapida' });
let area: HTMLTextAreaElement;
let nota: HTMLInputElement;
let detectado: HTMLElement;

function pintar(): void {
  contenedor.replaceChildren();
  const cabecera = h('header', { class: 'rap-cab', 'data-tauri-drag-region': '', onmousedown: arrastrar });
  cabecera.appendChild(logo('marca-logo'));
  cabecera.appendChild(h('h1', { text: 'Guardar miga' }));
  const segmentado = h('div', { class: 'segmentado' });
  for (const item of MODOS) {
    segmentado.appendChild(
      h('button', {
        'aria-pressed': modo === item.id ? 'true' : 'false',
        text: item.texto,
        onclick: () => {
          modo = item.id;
          pintar();
          area.focus();
          area.setSelectionRange(area.value.length, area.value.length);
        },
      }),
    );
  }
  cabecera.appendChild(segmentado);
  contenedor.appendChild(cabecera);

  const cuerpo = h('div', { class: 'rap-cuerpo' });
  area = h('textarea', {
    class: 'rap-texto',
    value: textos[modo],
    oninput: () => {
      textos[modo] = area.value;
      actualizarDeteccion();
    },
    onkeydown: (evento: KeyboardEvent) => {
      if (evento.key === 'Enter' && !evento.shiftKey) {
        evento.preventDefault();
        void guardar();
      }
    },
  }) as HTMLTextAreaElement;
  cuerpo.appendChild(area);
  nota = h('input', {
    class: 'rap-nota',
    placeholder: 'Nota (opcional)',
    value: notaTexto,
    oninput: () => {
      notaTexto = nota.value;
    },
    onkeydown: (evento: KeyboardEvent) => {
      if (evento.key === 'Enter') {
        evento.preventDefault();
        void guardar();
      }
    },
  }) as HTMLInputElement;
  cuerpo.appendChild(nota);
  contenedor.appendChild(cuerpo);

  const pie = h('footer', { class: 'rap-pie' });
  detectado = h('div', { class: 'detectado' });
  pie.appendChild(detectado);
  pie.appendChild(
    h(
      'button',
      {
        class: 'btn fantasma peq',
        onclick: () => void api.cerrarRapida(),
      },
      h('kbd', { text: 'Esc' }),
      ' cancelar',
    ),
  );
  pie.appendChild(h('button', { class: 'btn primario', onclick: () => void guardar() }, 'Guardar', h('kbd', { text: '↵' })));
  contenedor.appendChild(pie);

  raiz.replaceChildren(contenedor);
  actualizarDeteccion();
  area.focus();
  area.setSelectionRange(area.value.length, area.value.length);
}

function actualizarDeteccion(): void {
  const texto = area.value.trim();
  detectado.replaceChildren();
  const vistos = new Set<string>();
  for (const clave of detectarClaves(texto, get().ajustes.prefijosJira)) {
    vistos.add(clave.texto);
    detectado.appendChild(
      h('span', { class: `motivo${clave.fuerte ? ' fuerte' : ''}`, text: clave.texto }),
    );
  }
  for (const app of get().ajustes.apps ?? []) {
    if (texto.includes(app) && !vistos.has(app)) {
      vistos.add(app);
      detectado.appendChild(h('span', { class: 'motivo', text: app }));
    }
  }
  area.classList.toggle('es-enlace', esUrlUnica(texto));
}

async function guardar(): Promise<void> {
  const generacion = generacionCaptura;
  const texto = area.value.trim();
  if (!texto && !nota.value.trim()) return;
  const extra = nota.value.trim();
  let nueva: NuevaEntrada;
  if (esUrlUnica(texto)) {
    nueva = {
      tipo: 'enlace',
      origen: 'global',
      url: texto,
      titulo: null,
      seleccion: null,
      nota: extra || null,
    };
  } else if (modo === 'nota') {
    nueva = {
      tipo: 'nota',
      origen: 'global',
      url: null,
      titulo: null,
      seleccion: null,
      nota: [texto, extra].filter(Boolean).join('\n'),
    };
  } else {
    nueva = {
      tipo: 'texto',
      origen: 'global',
      url: null,
      titulo: null,
      seleccion: texto || null,
      nota: extra || null,
    };
  }
  try {
    const resultado = await api.crearEntrada(nueva);
    if (generacion !== generacionCaptura) return;
    textos.seleccion = '';
    textos.portapapeles = '';
    textos.nota = '';
    notaTexto = '';
    mostrarHecho(resultado);
  } catch (error) {
    if (generacion === generacionCaptura) {
      mostrarAviso(String(error), { mal: true });
    }
  }
}

function mostrarHecho(resultado: ResultadoCaptura): void {
  const hecho = h('div', { class: 'rap-hecho' });
  hecho.appendChild(h('div', { class: 'check' }, icono('check', 'l')));
  hecho.appendChild(h('h2', { text: resultado.duplicada ? 'Ya estaba en el diario' : 'Guardada' }));
  const detalle = [resultado.clave, resultado.cambios ? `${resultado.cambios} cambios` : null, resultado.relacionadas ? `${resultado.relacionadas} relacionadas` : null]
    .filter(Boolean)
    .join(' · ');
  hecho.appendChild(h('p', { text: detalle || resultado.titulo }));
  hecho.appendChild(
    h(
      'button',
      {
        class: 'btn primario',
        text: 'Abrir en migas',
        onclick: () => void api.mostrarPrincipal(resultado.id),
      },
    ),
  );
  contenedor.replaceChildren(hecho);
  if (cierreGuardado !== null) window.clearTimeout(cierreGuardado);
  cierreGuardado = window.setTimeout(() => {
    cierreGuardado = null;
    void api.cerrarRapida();
  }, 900);
}

function aplicarCaptura(nueva: CapturaRapida): void {
  const focoEnNota = document.activeElement === nota;
  generacionCaptura += 1;
  if (cierreGuardado !== null) {
    window.clearTimeout(cierreGuardado);
    cierreGuardado = null;
  }
  textos.seleccion = nueva.primaria ?? '';
  textos.portapapeles = nueva.portapapeles ?? '';
  textos.nota = '';
  notaTexto = '';
  modo = nueva.primaria ? 'seleccion' : nueva.portapapeles ? 'portapapeles' : 'nota';
  pintar();
  if (focoEnNota) nota.focus();
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
  } catch {
    // sin backend
  }
  try {
    const inicial = await api.capturaPendiente();
    textos.seleccion = inicial.primaria ?? '';
    textos.portapapeles = inicial.portapapeles ?? '';
    modo = inicial.primaria ? 'seleccion' : inicial.portapapeles ? 'portapapeles' : 'nota';
  } catch {
    modo = 'nota';
  }
  pintar();
  window.addEventListener('keydown', (evento) => {
    if (evento.key === 'Escape') {
      evento.preventDefault();
      void api.cerrarRapida();
    }
  });
  try {
    const { esTauri } = await import('./dom');
    if (esTauri()) {
      await eventos.rapida((nueva) => aplicarCaptura(nueva));
    }
  } catch {
    // sin Tauri (mock)
  }
}

void iniciar();
