import type { EntradaVista } from '../types';
import { h, icono } from '../dom';
import {
  fuenteIcono,
  fuenteTexto,
  resumenCambios,
  textoSeleccion,
  trozosDeFragmento,
  urlCorta,
} from '../formato';
import {
  abrirEntrada,
  borrar,
  borrarDefinitivo,
  copiarEntrada,
  fijar,
  get,
  restaurarEntrada,
  seleccionar,
} from '../estado';

export function pintarMiga(entrada: EntradaVista, seleccionada: boolean): HTMLElement {
  const enPapelera = get().vista === 'papelera';
  const articulo = h('article', {
    class: `miga${entrada.cambios.length ? ' con-cambios' : ''}`,
    'data-fuente': entrada.fuente,
    role: 'option',
    'aria-selected': seleccionada ? 'true' : 'false',
    onclick: (evento: Event) => {
      const objetivo = evento.target as HTMLElement | null;
      if (objetivo?.closest('button, a')) return;
      void seleccionar(entrada.id);
    },
    ondblclick: () => {
      if (entrada.url) void abrirEntrada(entrada.id);
      else void copiarEntrada(entrada);
    },
  });

  articulo.appendChild(h('time', { class: 'miga-hora', text: entrada.hora }));
  articulo.appendChild(h('div', { class: 'miga-eje' }, h('span', { class: 'miga-punto' })));

  const cuerpo = h('div', { class: 'miga-cuerpo' });
  const cabecera = h('header', { class: 'miga-cab' });
  const fuente = h('span', { class: 'fuente' }, icono(fuenteIcono(entrada), 's'), fuenteTexto(entrada));
  cabecera.appendChild(fuente);
  if (entrada.clave) cabecera.appendChild(h('span', { class: 'clave', text: entrada.clave }));
  if (entrada.estado) cabecera.appendChild(h('span', { class: 'estado', text: entrada.estado }));
  if (entrada.app) cabecera.appendChild(h('span', { class: 'app-tag', text: entrada.app }));
  if (entrada.fijada) cabecera.appendChild(icono('pin', 's fija'));
  cuerpo.appendChild(cabecera);

  if (entrada.tipo !== 'texto' && entrada.tipo !== 'nota') {
    cuerpo.appendChild(h('h3', { class: 'miga-titulo', text: entrada.titulo }));
  }
  if (entrada.fragmento?.length) {
    const fragmento = h('div', { class: 'fragmento' });
    fragmento.appendChild(trozosDeFragmento(entrada.fragmento));
    cuerpo.appendChild(fragmento);
  }
  const cita = textoSeleccion(entrada);
  if (cita) cuerpo.appendChild(h('blockquote', { class: 'miga-cita', text: cita }));
  if (entrada.ultimaNota && entrada.tipo !== 'nota') {
    cuerpo.appendChild(
      h('div', { class: 'miga-nota' }, icono('corner-down-right', 's'), h('span', { text: entrada.ultimaNota })),
    );
  }

  const pie = h('footer', { class: 'miga-pie' });
  if (entrada.cambios.length) {
    pie.appendChild(
      h(
        'span',
        { class: 'cambio-pill' },
        h('span', { class: 'luz' }),
        h('span', { text: resumenCambios(entrada.cambios) }),
      ),
    );
  }
  if (entrada.notas > 0) {
    pie.appendChild(
      h('span', { class: 'meta', title: `${entrada.notas} notas` }, icono('sticky-note', 's'), String(entrada.notas)),
    );
  }
  if (entrada.relacionadas > 0) {
    pie.appendChild(
      h(
        'span',
        { class: 'meta', title: `${entrada.relacionadas} relacionadas` },
        icono('waypoints', 's'),
        String(entrada.relacionadas),
      ),
    );
  }
  if (entrada.capturas > 1) {
    pie.appendChild(
      h('span', { class: 'meta' }, icono('repeat', 's'), `${entrada.capturas} veces`),
    );
  }
  cuerpo.appendChild(pie);
  articulo.appendChild(cuerpo);

  const acciones = h('div', { class: 'miga-acciones' });
  if (entrada.url) {
    acciones.appendChild(
      h('button', {
        class: 'accion',
        title: 'Abrir (Enter)',
        'aria-label': 'Abrir en el navegador',
        onclick: (evento: Event) => {
          evento.stopPropagation();
          void abrirEntrada(entrada.id);
        },
      }, icono('arrow-up-right')),
    );
  } else {
    acciones.appendChild(
      h('button', {
        class: 'accion',
        title: 'Copiar (C)',
        'aria-label': 'Copiar texto',
        onclick: (evento: Event) => {
          evento.stopPropagation();
          void copiarEntrada(entrada);
        },
      }, icono('copy')),
    );
  }
  acciones.appendChild(
    h('button', {
      class: 'accion',
      title: 'Nota (N)',
      'aria-label': 'Escribir una nota',
      onclick: async (evento: Event) => {
        evento.stopPropagation();
        await seleccionar(entrada.id);
        document.querySelector<HTMLTextAreaElement>('.compositor textarea')?.focus();
      },
    }, icono('sticky-note')),
  );
  if (enPapelera) {
    acciones.appendChild(
      h('button', {
        class: 'accion',
        title: 'Restaurar',
        'aria-label': 'Restaurar',
        onclick: (evento: Event) => {
          evento.stopPropagation();
          void restaurarEntrada(entrada.id);
        },
      }, icono('undo-2')),
    );
    acciones.appendChild(
      h('button', {
        class: 'accion',
        title: 'Borrar para siempre (Supr)',
        'aria-label': 'Borrar para siempre (Supr)',
        onclick: (evento: Event) => {
          evento.stopPropagation();
          borrarDefinitivo(entrada.id);
        },
      }, icono('trash-2')),
    );
  } else {
    acciones.appendChild(
      h('button', {
        class: 'accion',
        title: 'Fijar (F)',
        'aria-label': entrada.fijada ? 'Desfijar' : 'Fijar',
        'aria-pressed': entrada.fijada ? 'true' : 'false',
        onclick: (evento: Event) => {
          evento.stopPropagation();
          void fijar(entrada.id, !entrada.fijada);
        },
      }, icono('pin')),
    );
    acciones.appendChild(
      h('button', {
        class: 'accion',
        title: 'A la papelera (Supr)',
        'aria-label': 'Mover a la papelera',
        onclick: (evento: Event) => {
          evento.stopPropagation();
          void borrar(entrada.id);
        },
      }, icono('trash-2')),
    );
  }
  articulo.appendChild(acciones);

  if (entrada.url) articulo.title = urlCorta(entrada.url);
  return articulo;
}
