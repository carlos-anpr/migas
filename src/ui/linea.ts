import { h, icono, logo } from '../dom';
import {
  alternarFuente,
  diaAnteriorConEntradas,
  diaSiguienteConEntradas,
  generarResumenDia,
  get,
  irDia,
  irHoy,
  limpiarFuentes,
  quitarBusqueda,
  vaciarPapelera,
  type EstadoUI,
} from '../estado';
import { diaPartes, etiquetaDia, fuenteCorta, pluralMigas } from '../formato';
import { pintarMiga } from './miga';
import type { EntradaVista, Fuente, Vista } from '../types';

const TITULOS: Record<Exclude<Vista, 'dia'>, string> = {
  cambios: 'Con cambios',
  fijadas: 'Fijadas',
  todo: 'Todo',
  papelera: 'Papelera',
};

export function pintarLinea(): HTMLElement {
  const estado = get();
  const linea = h('main', { class: 'linea' });
  linea.appendChild(pintarCabecera(estado));
  linea.appendChild(pintarLista(estado));
  return linea;
}

function pintarCabecera(estado: EstadoUI): HTMLElement {
  const cabecera = h('div', { class: 'linea-cab' });
  const titulo = h('div', { class: 'linea-titulo' });
  if (estado.vista === 'dia') {
    const partes = diaPartes(estado.dia);
    titulo.appendChild(
      h('h1', {}, h('span', { class: 'dia-sem', text: partes.semana }), ' ', partes.resto),
    );
  } else {
    titulo.appendChild(
      h('h1', { text: estado.texto ? `“${estado.texto}”` : TITULOS[estado.vista] }),
    );
  }
  const cambios = estado.entradas.filter((entrada) => entrada.cambios.length).length;
  const sub = h('div', { class: 'sub' });
  sub.appendChild(h('span', { text: pluralMigas(estado.total) }));
  if (cambios > 0) {
    sub.appendChild(document.createTextNode(' · '));
    sub.appendChild(h('b', { text: `${cambios} con cambios` }));
  }
  titulo.appendChild(sub);

  const navegacion = h('div', { class: 'nav-dia' });
  if (estado.vista === 'dia') {
    const anterior = diaAnteriorConEntradas();
    const siguiente = diaSiguienteConEntradas();
    navegacion.appendChild(
      h(
        'button',
        {
          class: 'btn fantasma peq',
          title: 'Día anterior [',
          'aria-label': 'Día anterior',
          disabled: !anterior,
          onclick: () => anterior && void irDia(anterior),
        },
        icono('chevron-left', 's'),
      ),
    );
    navegacion.appendChild(
      h('button', { class: 'btn peq', title: 'Hoy (T)', onclick: () => void irHoy(), text: 'Hoy' }),
    );
    navegacion.appendChild(
      h(
        'button',
        {
          class: 'btn fantasma peq',
          title: 'Día siguiente ]',
          'aria-label': 'Día siguiente',
          disabled: !siguiente,
          onclick: () => siguiente && void irDia(siguiente),
        },
        icono('chevron-right', 's'),
      ),
    );
    if (estado.estadoApp?.iaDisponible) {
      navegacion.appendChild(
        h(
          'button',
          {
            class: 'btn fantasma peq',
            title: 'Resumen del día con IA',
            disabled: estado.resumiendo,
            onclick: () => void generarResumenDia(),
          },
          icono('sparkles', 's'),
          estado.resumiendo ? 'Resumiendo…' : 'Resumir',
        ),
      );
    }
  } else if (estado.texto) {
    navegacion.appendChild(
      h(
        'button',
        {
          class: 'btn fantasma peq',
          title: 'Quitar la búsqueda (Esc)',
          'aria-label': 'Quitar la búsqueda',
          onclick: () => void quitarBusqueda(),
        },
        icono('x', 's'),
        'Quitar búsqueda',
      ),
    );
  } else if (estado.vista === 'papelera' && estado.total > 0) {
    navegacion.appendChild(
      h(
        'button',
        {
          class: 'btn fantasma peq',
          title: 'Vaciar la papelera',
          'aria-label': 'Vaciar la papelera',
          onclick: () => vaciarPapelera(),
        },
        icono('trash-2', 's'),
        'Vaciar papelera',
      ),
    );
  }
  titulo.appendChild(navegacion);
  cabecera.appendChild(titulo);

  const filtros = h('div', { class: 'filtros' });
  const todas = h(
    'button',
    {
      class: 'chip',
      'aria-pressed': estado.fuentes.length === 0 ? 'true' : 'false',
      onclick: () => void limpiarFuentes(),
    },
    'Todas',
    h('span', { class: 'n', text: String(estado.total) }),
  );
  filtros.appendChild(todas);
  filtros.appendChild(h('span', { class: 'sep' }));
  const fuentes: Fuente[] = ['jira', 'gitlab', 'fs', 'web', 'texto'];
  for (const fuente of fuentes) {
    const activa = estado.fuentes.includes(fuente);
    filtros.appendChild(
      h(
        'button',
        {
          class: 'chip',
          'data-fuente': fuente,
          'aria-pressed': activa ? 'true' : 'false',
          onclick: () => void alternarFuente(fuente),
        },
        h('span', { class: 'bola' }),
        fuenteCorta(fuente),
        h('span', { class: 'n', text: String(estado.fuentesConteo[fuente] ?? 0) }),
      ),
    );
  }
  cabecera.appendChild(filtros);
  return cabecera;
}

function pintarLista(estado: EstadoUI): HTMLElement {
  const lista = h('div', { class: 'lista', role: 'listbox', 'aria-label': 'Migas' });
  if (estado.vista === 'dia' && estado.resumen) {
    lista.appendChild(pintarResumen(estado));
  }
  if (mostrarBanner(estado)) lista.appendChild(pintarBanner());
  if (estado.entradas.length === 0) {
    lista.appendChild(pintarVacio(estado));
    return lista;
  }
  const multiDia = estado.vista !== 'dia';
  let diaActual = '';
  for (const entrada of estado.entradas) {
    if (multiDia && entrada.dia !== diaActual) {
      diaActual = entrada.dia;
      lista.appendChild(
        h(
          'div',
          { class: 'grupo-dia' },
          etiquetaDia(entrada.dia),
          h('span', { text: pluralMigas(contarDia(estado.entradas, entrada.dia)) }),
        ),
      );
    }
    lista.appendChild(pintarMiga(entrada, estado.seleccion === entrada.id));
  }
  return lista;
}

function pintarResumen(estado: EstadoUI): HTMLElement {
  const banner = h('div', { class: 'banner resumen-dia' });
  banner.appendChild(h('div', { class: 'ico-caja' }, icono('sparkles', 'l')));
  const texto = h('div');
  texto.appendChild(h('h4', { text: 'Resumen del día' }));
  const lineas = (estado.resumen?.texto ?? '')
    .split('\n')
    .map((linea) => linea.trim())
    .filter(Boolean);
  for (const linea of lineas.length ? lineas : ['Sin contenido']) {
    texto.appendChild(h('p', { text: linea }));
  }
  banner.appendChild(texto);
  const acciones = h('div');
  acciones.appendChild(
    h(
      'button',
      {
        class: 'btn peq',
        disabled: estado.resumiendo,
        text: estado.resumiendo ? 'Resumiendo…' : 'Regenerar',
        onclick: () => void generarResumenDia(),
      },
    ),
  );
  banner.appendChild(acciones);
  return banner;
}

function contarDia(entradas: EntradaVista[], dia: string): number {
  return entradas.filter((entrada) => entrada.dia === dia).length;
}

function mostrarBanner(estado: EstadoUI): boolean {
  const integraciones = estado.estadoApp?.integraciones;
  if (!integraciones) return false;
  // Solo tiene sentido pedir la conexión si no hay host Y la extensión nunca ha hablado.
  if (integraciones.hostRegistrado || integraciones.extensionVista) return false;
  return localStorage.getItem('migas-banner') !== 'cerrado';
}

function pintarBanner(): HTMLElement {
  const banner = h('div', { class: 'banner' });
  banner.appendChild(h('div', { class: 'ico-caja' }, icono('plug', 'l')));
  const texto = h('div');
  texto.appendChild(h('h4', { text: 'Conecta el navegador — 2 pasos' }));
  texto.appendChild(
    h('p', {
      text: 'Instala el host nativo y carga la extensión para guardar páginas con Ctrl Ctrl.',
    }),
  );
  banner.appendChild(texto);
  const acciones = h('div');
  acciones.appendChild(
    h('button', {
      class: 'btn peq primario',
      text: 'Configurar',
      onclick: () => {
        const evento = new CustomEvent('migas:ajustes', { detail: 'conexion' });
        window.dispatchEvent(evento);
      },
    }),
  );
  acciones.appendChild(
    h('button', {
      class: 'btn-ventana',
      title: 'Cerrar aviso',
      'aria-label': 'Cerrar aviso',
      onclick: () => {
        localStorage.setItem('migas-banner', 'cerrado');
        banner.remove();
      },
    }, icono('x')),
  );
  banner.appendChild(acciones);
  return banner;
}

function pintarVacio(estado: EstadoUI): HTMLElement {
  const vacio = h('div', { class: 'vacio' });
  vacio.appendChild(logo('marca-logo'));
  if (estado.texto) {
    vacio.appendChild(h('h3', { text: `Nada coincide con “${estado.texto}”` }));
    vacio.appendChild(h('p', { text: 'Prueba con otra palabra o quita los filtros.' }));
    return vacio;
  }
  switch (estado.vista) {
    case 'dia':
      vacio.appendChild(h('h3', { text: 'Nada guardado este día' }));
      vacio.appendChild(
        h(
          'p',
          {},
          'Pulsa ',
          h('span', { class: 'gesto' }, h('kbd', { text: 'Ctrl' }), h('kbd', { text: 'Ctrl' })),
          ' en cualquier página o selección para guardarla.',
        ),
      );
      break;
    case 'cambios':
      vacio.appendChild(h('h3', { text: 'Todo al día' }));
      vacio.appendChild(
        h('p', { text: 'Te avisaré cuando alguien conteste o cambie un estado.' }),
      );
      break;
    case 'fijadas':
      vacio.appendChild(h('h3', { text: 'Nada fijado' }));
      vacio.appendChild(h('p', { text: 'Pulsa F sobre una miga para fijarla.' }));
      break;
    case 'papelera':
      vacio.appendChild(h('h3', { text: 'La papelera está vacía' }));
      vacio.appendChild(h('p', { text: 'Lo que borres aparecerá aquí durante 30 días.' }));
      break;
    case 'todo':
      vacio.appendChild(h('h3', { text: 'Todavía no hay migas' }));
      vacio.appendChild(h('p', { text: 'Guarda una página o una selección para empezar.' }));
      break;
  }
  return vacio;
}
