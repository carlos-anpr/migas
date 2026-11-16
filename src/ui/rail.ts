import { h, icono } from '../dom';
import {
  get,
  irDia,
  irHoy,
  irVista,
  set,
  type EstadoUI,
} from '../estado';
import { diaLocal, etiquetaDia, hace, hoy, mesLargo, nivelDia, pluralMigas } from '../formato';
import type { Fuente, Vista } from '../types';

interface EntradaNav {
  vista: Vista;
  icono: string;
  texto: string;
  cuenta?: number;
  viva?: boolean;
}

export function pintarRail(abrirAjustes: (seccion?: string) => void): HTMLElement {
  const estado = get();
  const rail = h('nav', { class: 'rail' });

  const nav = h('div', { class: 'nav' });
  const hoyResumen = estado.dias.find((dia) => dia.dia === hoy());
  const totalTodo = estado.dias.reduce((suma, dia) => suma + dia.total, 0);
  const items: EntradaNav[] = [
    { vista: 'dia', icono: 'sun', texto: 'Hoy', cuenta: hoyResumen?.total ?? 0 },
    { vista: 'cambios', icono: 'bell-dot', texto: 'Con cambios', cuenta: estado.estadoApp?.cambios ?? 0, viva: (estado.estadoApp?.cambios ?? 0) > 0 },
    { vista: 'fijadas', icono: 'pin', texto: 'Fijadas', cuenta: estado.fijadasTotal },
    { vista: 'todo', icono: 'layers', texto: 'Todo', cuenta: totalTodo },
    { vista: 'papelera', icono: 'trash-2', texto: 'Papelera' },
  ];
  for (const item of items) {
    const activa =
      item.vista === 'dia'
        ? estado.vista === 'dia' && estado.dia === hoy()
        : estado.vista === item.vista;
    const enlace = h(
      'a',
      {
        class: 'nav-item',
        href: '#',
        'aria-current': activa ? 'true' : undefined,
        onclick: (evento: Event) => {
          evento.preventDefault();
          if (item.vista === 'dia') void irHoy();
          else void irVista(item.vista);
        },
      },
      icono(item.icono),
      item.texto,
    );
    if (item.cuenta !== undefined) {
      enlace.appendChild(
        h('span', { class: `cuenta${item.viva ? ' viva' : ''}`, text: String(item.cuenta) }),
      );
    }
    nav.appendChild(enlace);
  }
  rail.appendChild(nav);
  rail.appendChild(pintarCalendario(estado));
  rail.appendChild(pintarDias(estado));
  rail.appendChild(pintarPie(estado, abrirAjustes));
  return rail;
}

function pintarCalendario(estado: EstadoUI): HTMLElement {
  const seccion = h('section', { class: 'calendario' });
  const [anio, mes] = estado.mesCalendario.split('-').map(Number);
  const titulo = h('div', { class: 'rail-titulo' });
  titulo.appendChild(h('span', { class: 'cal-mes', text: mesLargo(estado.mesCalendario) }));
  const flechas = h('span', { class: 'flechas' });
  flechas.appendChild(
    h(
      'button',
      {
        class: 'mini-btn',
        title: 'Mes anterior',
        'aria-label': 'Mes anterior',
        onclick: () => {
          const fecha = new Date(anio, (mes ?? 1) - 2, 1);
          set({ mesCalendario: diaLocal(fecha).slice(0, 7) });
        },
      },
      icono('chevron-left', 's'),
    ),
  );
  flechas.appendChild(
    h(
      'button',
      {
        class: 'mini-btn',
        title: 'Mes siguiente',
        'aria-label': 'Mes siguiente',
        onclick: () => {
          const fecha = new Date(anio, mes ?? 1, 1);
          set({ mesCalendario: diaLocal(fecha).slice(0, 7) });
        },
      },
      icono('chevron-right', 's'),
    ),
  );
  titulo.appendChild(flechas);
  seccion.appendChild(titulo);

  seccion.appendChild(
    h(
      'div',
      { class: 'cal-semana' },
      ...['L', 'M', 'X', 'J', 'V', 'S', 'D'].map((letra) => h('span', { text: letra })),
    ),
  );

  const inicio = new Date(anio, (mes ?? 1) - 1, 1);
  const desplazamiento = (inicio.getDay() + 6) % 7;
  const rejilla = h('div', { class: 'cal-rejilla' });
  const resumenPorDia = new Map(estado.dias.map((dia) => [dia.dia, dia]));
  for (let indice = 0; indice < 42; indice += 1) {
    const fecha = new Date(anio, (mes ?? 1) - 1, 1 - desplazamiento + indice);
    const dia = diaLocal(fecha);
    const resumen = resumenPorDia.get(dia);
    const fuera = fecha.getMonth() !== (mes ?? 1) - 1;
    const clases = ['cal-dia'];
    if (fuera) clases.push('fuera');
    if (resumen && resumen.cambios > 0) clases.push('con-cambios');
    if (dia === hoy()) clases.push('hoy');
    const boton = h('button', {
      class: clases.filter(Boolean).join(' '),
      'data-nivel': resumen && resumen.total > 0 ? String(nivelDia(resumen.total)) : undefined,
      'aria-selected': dia === estado.dia && estado.vista === 'dia' ? 'true' : undefined,
      title: resumen ? pluralMigas(resumen.total) : undefined,
      onclick: () => void irDia(dia),
      text: String(fecha.getDate()),
    });
    rejilla.appendChild(boton);
  }
  seccion.appendChild(rejilla);
  return seccion;
}

function pintarDias(estado: EstadoUI): HTMLElement {
  const seccion = h('section');
  seccion.appendChild(h('div', { class: 'rail-titulo', text: 'Días recientes' }));
  const dias = h('div', { class: 'dias' });
  for (const resumen of estado.dias.slice(0, 7)) {
    const activo = estado.vista === 'dia' && estado.dia === resumen.dia;
    const enlace = h(
      'a',
      {
        class: 'dia-item',
        href: '#',
        'aria-current': activo ? 'true' : undefined,
        onclick: (evento: Event) => {
          evento.preventDefault();
          void irDia(resumen.dia);
        },
      },
      h('span', { text: etiquetaDia(resumen.dia) }),
      h(
        'span',
        { class: 'n' },
        resumen.cambios > 0 ? h('i', { class: 'punto' }) : null,
        String(resumen.total),
      ),
    );
    dias.appendChild(enlace);
  }
  seccion.appendChild(dias);
  return seccion;
}

function nombreFuente(fuente: Fuente): string {
  switch (fuente) {
    case 'jira':
      return 'Jira';
    case 'gitlab':
      return 'GitLab';
    case 'fs':
      return 'Freshservice';
    case 'web':
      return 'Web';
    case 'texto':
      return 'Texto';
    case 'nota':
      return 'Nota';
  }
}

function pintarPie(estado: EstadoUI, abrirAjustes: (seccion?: string) => void): HTMLElement {
  const pie = h('div', { class: 'rail-pie' });
  const integraciones = estado.estadoApp?.integraciones;
  const extension = integraciones?.extensionVista;
  const reciente = extension
    ? Date.now() - new Date(extension).getTime() < 24 * 60 * 60 * 1000
    : false;
  pie.appendChild(
    h(
      'div',
      {
        class: `integ ${reciente ? 'ok' : 'alerta'}`,
        title: extension ? `Última conexión ${hace(extension)}` : 'La extensión aún no ha hablado',
        onclick: () => abrirAjustes('conexion'),
      },
      h('span', { class: 'luz' }),
      h('b', { text: 'Navegador' }),
      reciente ? ' conectado' : ' sin conectar',
    ),
  );

  const doble = integraciones?.dobleCtrl ?? 'desactivado';
  const dobleOk = doble === 'activo';
  pie.appendChild(
    h(
      'div',
      { class: `integ ${dobleOk ? 'ok' : 'alerta'}` },
      h('span', { class: 'luz' }),
      h('b', { text: 'Doble Ctrl' }),
      ` ${doble}`,
    ),
  );

  for (const fuente of estado.estadoApp?.fuentes ?? []) {
    if (fuente.configurada) continue;
    pie.appendChild(
      h(
        'div',
        { class: 'integ alerta', onclick: () => abrirAjustes('fuentes') },
        h('span', { class: 'luz' }),
        h('b', { text: nombreFuente(fuente.fuente) }),
        ' sin clave',
      ),
    );
  }
  return pie;
}
