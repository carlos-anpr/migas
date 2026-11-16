import type { Cambio, Comentario, DetalleEntrada, Nota, RecursoVista, Relacion } from '../types';
import { h, icono, logo } from '../dom';
import {
  abrirEntrada,
  borradorDeNota,
  borrar,
  borrarDefinitivo,
  borrarNota,
  copiar,
  copiarEntrada,
  crearNota,
  descartar,
  editarNota,
  editarTitulo,
  eliminarBorradorDeNota,
  fijar,
  get,
  guardarBorradorDeNota,
  marcarVisto,
  recargar,
  restaurarEntrada,
  saltarA,
  seguir,
  set,
  type EstadoUI,
} from '../estado';
import {
  diaHora,
  etiquetaDia,
  fuenteIcono,
  fuenteTexto,
  hace,
  iniciales,
  urlCorta,
} from '../formato';

const comentariosAbiertos = new Set<number>();
const ultimosAbiertos = new Set<number>();

export function pintarDetalle(estado: EstadoUI): HTMLElement {
  const detalle = estado.detalle;
  if (
    !detalle ||
    estado.seleccion === null ||
    detalle.entrada.id !== estado.seleccion
  ) {
    const aside = h('aside', { class: 'detalle' });
    aside.appendChild(pintarVacio());
    return aside;
  }
  const entrada = detalle.entrada;
  const enPapelera = get().vista === 'papelera';
  const aside = h('aside', { class: 'detalle', 'data-fuente': entrada.fuente });
  const dentro = h('div', { class: 'det-dentro' });

  const cabecera = h('div', { class: 'det-cab' });
  cabecera.appendChild(
    h('span', { class: 'fuente' }, icono(fuenteIcono(entrada), 's'), fuenteTexto(entrada)),
  );
  if (entrada.clave) cabecera.appendChild(h('span', { class: 'clave', text: entrada.clave }));
  if (entrada.estado) cabecera.appendChild(h('span', { class: 'estado', text: entrada.estado }));
  const acciones = h('div', { class: 'acciones' });
  if (enPapelera) {
    acciones.appendChild(
      h(
        'button',
        {
          class: 'accion',
          title: 'Restaurar',
          'aria-label': 'Restaurar',
          onclick: () => void restaurarEntrada(entrada.id),
        },
        icono('undo-2'),
      ),
    );
    acciones.appendChild(
      h(
        'button',
        {
          class: 'accion',
          title: 'Borrar para siempre (Supr)',
          'aria-label': 'Borrar para siempre (Supr)',
          onclick: () => borrarDefinitivo(entrada.id),
        },
        icono('trash-2'),
      ),
    );
  } else {
    acciones.appendChild(
      h(
        'button',
        {
          class: 'accion',
          title: 'Fijar (F)',
          'aria-label': entrada.fijada ? 'Desfijar' : 'Fijar',
          'aria-pressed': entrada.fijada ? 'true' : 'false',
          onclick: () => void fijar(entrada.id, !entrada.fijada),
        },
        icono('pin'),
      ),
    );
    acciones.appendChild(
      h(
        'button',
        {
          class: 'accion',
          title: 'A la papelera (Supr)',
          'aria-label': 'Mover a la papelera',
          onclick: () => void borrar(entrada.id),
        },
        icono('trash-2'),
      ),
    );
  }
  acciones.appendChild(
    h(
      'button',
      {
        class: 'accion',
        title: 'Cerrar panel (Esc)',
        'aria-label': 'Cerrar panel',
        onclick: () => set({ detalleAbierto: false }),
      },
      icono('panel-right-close'),
    ),
  );
  cabecera.appendChild(acciones);
  dentro.appendChild(cabecera);

  dentro.appendChild(pintarTitulo(entrada));
  if (entrada.url) {
    const url = h(
      'a',
      {
        class: 'det-url',
        href: entrada.url,
        onclick: (evento: Event) => {
          evento.preventDefault();
          void abrirEntrada(entrada.id);
        },
      },
      icono('link-2', 's'),
      h('span', { text: urlCorta(entrada.url) }),
    );
    dentro.appendChild(url);
  }

  const botones = h('div', { class: 'det-botones' });
  if (entrada.url) {
    botones.appendChild(
      h(
        'button',
        { class: 'btn primario', onclick: () => void abrirEntrada(entrada.id) },
        icono('arrow-up-right'),
        'Abrir',
        h('kbd', { text: '↵' }),
      ),
    );
    botones.appendChild(
      h(
        'button',
        { class: 'btn', onclick: () => void copiar(entrada.url ?? '') },
        icono('copy', 's'),
        'Copiar enlace',
      ),
    );
  } else {
    botones.appendChild(
      h(
        'button',
        { class: 'btn primario', onclick: () => void copiarEntrada(entrada) },
        icono('copy', 's'),
        'Copiar texto',
      ),
    );
  }
  if (enPapelera) {
    botones.appendChild(
      h(
        'button',
        { class: 'btn', onclick: () => void restaurarEntrada(entrada.id) },
        icono('undo-2', 's'),
        'Restaurar',
      ),
    );
    botones.appendChild(
      h(
        'button',
        {
          class: 'btn',
          title: 'Borrar para siempre (Supr)',
          onclick: () => borrarDefinitivo(entrada.id),
        },
        icono('trash-2', 's'),
        'Borrar para siempre',
      ),
    );
  }
  dentro.appendChild(botones);

  if (detalle.recurso) dentro.appendChild(pintarRecurso(entrada.id, detalle.recurso));
  if (detalle.seleccion) {
    const sec = h('section', { class: 'sec' });
    const titulo = h('div', { class: 'sec-titulo' }, 'Selección');
    titulo.appendChild(
      h(
        'button',
        {
          class: 'btn fantasma peq',
          title: 'Copiar selección',
          onclick: () => void copiar(detalle.seleccion ?? ''),
        },
        icono('copy', 's'),
        'Copiar',
      ),
    );
    sec.appendChild(titulo);
    sec.appendChild(h('blockquote', { class: 'cita-grande', text: detalle.seleccion }));
    dentro.appendChild(sec);
  }

  dentro.appendChild(pintarNotas(detalle));
  dentro.appendChild(pintarRelacionadas(entrada.id, detalle));
  dentro.appendChild(pintarHistorial(detalle));
  aside.appendChild(dentro);
  aside.appendChild(pintarPie(entrada.id, detalle.recurso));
  return aside;
}

function pintarVacio(): HTMLElement {
  const vacio = h('div', { class: 'det-vacio' });
  vacio.appendChild(logo('marca-logo'));
  vacio.appendChild(h('h3', { text: 'Tu rastro, a mano' }));
  vacio.appendChild(h('p', { text: 'Elige una miga para ver el detalle y las notas.' }));
  const atajos = h('dl', { class: 'atajos' });
  const filas: Array<[string, string]> = [
    ['Ctrl K', 'Buscar'],
    ['↑ ↓', 'Mover'],
    ['Enter', 'Abrir'],
    ['Espacio', 'Detalle'],
    ['N', 'Nota'],
    ['F', 'Fijar'],
    ['E', 'Marcar visto'],
    ['C', 'Copiar'],
    ['Supr', 'Papelera'],
    ['[ ]', 'Día anterior / siguiente'],
    ['T', 'Hoy'],
    ['1…5', 'Vistas'],
    ['Ctrl N', 'Nueva nota'],
    ['Ctrl ,', 'Ajustes'],
  ];
  for (const [tecla, accion] of filas) {
    atajos.appendChild(h('dt', {}, h('kbd', { text: tecla })));
    atajos.appendChild(h('dd', { text: accion }));
  }
  vacio.appendChild(atajos);
  return vacio;
}

function pintarTitulo(entrada: DetalleEntrada['entrada']): HTMLElement {
  const titulo = h('h2', {
    class: 'det-titulo',
    title: 'Doble clic para editar',
    text: entrada.titulo,
    ondblclick: () => {
      const input = h('input', { class: 'det-titulo-input', value: entrada.titulo }) as HTMLInputElement;
      titulo.replaceWith(input);
      input.focus();
      input.select();
      let terminado = false;
      const cerrar = (guardar: boolean): void => {
        if (terminado) return;
        terminado = true;
        if (guardar && input.value.trim() && input.value.trim() !== entrada.titulo) {
          void editarTitulo(entrada.id, input.value.trim());
        } else {
          void recargar();
        }
      };
      input.addEventListener('keydown', (evento) => {
        if (evento.key === 'Enter') {
          evento.preventDefault();
          cerrar(true);
        } else if (evento.key === 'Escape') {
          evento.preventDefault();
          cerrar(false);
        }
      });
      input.addEventListener('blur', () => cerrar(true));
    },
  });
  return titulo;
}

function pintarRecurso(entradaId: number, recurso: RecursoVista): HTMLElement {
  const contenedor = h('div');
  if (recurso.cambios.length) {
    const caja = h('section', { class: 'cambios-caja' });
    const cabecera = h(
      'div',
      { class: 'cambios-cab' },
      icono('sparkle', 's'),
      `Desde tu última visita${recurso.visto ? ` · ${hace(recurso.visto)}` : ''}`,
    );
    cabecera.appendChild(
      h(
        'button',
        {
          class: 'btn peq',
          onclick: () => void marcarVisto(entradaId),
        },
        'Marcar visto',
        h('kbd', { text: 'E' }),
      ),
    );
    caja.appendChild(cabecera);
    for (const cambio of recurso.cambios) {
      const fila = filaCambio(cambio);
      if (fila) caja.appendChild(fila);
    }
    if (recurso.ultimos.length) caja.appendChild(pintarComentarios(recurso.ultimos, recurso.nuevos));
    contenedor.appendChild(caja);
  } else if (recurso.ultimos.length) {
    contenedor.appendChild(pintarUltimos(recurso.ultimos, recurso.nuevos));
  }
  return contenedor;
}

function pintarUltimos(ultimos: Comentario[], nuevos: string[]): HTMLElement {
  const sec = h('section', { class: 'sec' });
  const titulo = h('div', { class: 'sec-titulo' }, 'Últimos comentarios');
  const abierto = ultimosAbiertos.has(0);
  const contenido = h('div', { class: 'comentarios' });
  if (abierto) contenido.appendChild(pintarComentarios(ultimos, nuevos));
  const boton = h('button', {
    class: 'btn fantasma peq',
    text: abierto ? 'Ocultar' : 'Ver',
    onclick: () => {
      if (ultimosAbiertos.has(0)) ultimosAbiertos.delete(0);
      else ultimosAbiertos.add(0);
      void recargar();
    },
  });
  titulo.appendChild(boton);
  sec.appendChild(titulo);
  if (abierto) sec.appendChild(contenido);
  return sec;
}

function pintarComentarios(ultimos: Comentario[], nuevos: string[]): HTMLElement {
  const contenedor = h('div', { class: 'comentarios' });
  const limite = comentariosAbiertos.has(0) ? ultimos.length : Math.min(5, ultimos.length);
  for (const comentario of ultimos.slice(0, limite)) {
    contenedor.appendChild(pintarComentario(comentario, nuevos.includes(comentario.id)));
  }
  if (ultimos.length > 5) {
    contenedor.appendChild(
      h('button', {
        class: 'btn fantasma peq',
        text: comentariosAbiertos.has(0) ? 'Ver menos' : `Ver más (${ultimos.length - 5})`,
        onclick: () => {
          if (comentariosAbiertos.has(0)) comentariosAbiertos.delete(0);
          else comentariosAbiertos.add(0);
          void recargar();
        },
      }),
    );
  }
  return contenedor;
}

function pintarComentario(comentario: Comentario, nuevo: boolean): HTMLElement {
  const fila = h('div', { class: `comentario${nuevo ? ' nuevo' : ''}` });
  fila.appendChild(h('span', { class: 'avatar', text: iniciales(comentario.autor) }));
  const quien = h('div', { class: 'quien' });
  quien.appendChild(h('span', { text: comentario.mio ? 'Tú' : comentario.autor }));
  quien.appendChild(h('time', { text: diaHora(comentario.fecha) }));
  fila.appendChild(quien);
  fila.appendChild(h('div', { class: 'que', text: comentario.extracto }));
  return fila;
}

function filaCambio(cambio: Cambio): HTMLElement | null {
  const fila = (etiqueta: string, antes?: string | null, despues?: string | null): HTMLElement => {
    const elemento = h('div', { class: 'cambio-fila' }, icono('arrow-right-left', 's'), etiqueta);
    if (antes !== undefined) elemento.appendChild(h('span', { class: 'de', text: antes ?? '—' }));
    if (despues !== undefined) elemento.appendChild(h('span', { class: 'a', text: despues ?? '—' }));
    return elemento;
  };
  switch (cambio.tipo) {
    case 'comentarios':
      return null;
    case 'estado':
      return fila('Estado', cambio.antes, cambio.despues);
    case 'asignado':
      return fila('Asignado', cambio.antes, cambio.despues);
    case 'titulo':
      return fila('Título', cambio.antes, cambio.despues);
    case 'pipeline':
      return fila('Pipeline', cambio.antes, cambio.despues);
    case 'fusionada':
      return h('div', { class: 'cambio-fila' }, icono('git-merge', 's'), 'Fusionada');
    case 'etiquetas': {
      const partes: string[] = [];
      if (cambio.anadidas.length) partes.push(`+${cambio.anadidas.join(', ')}`);
      if (cambio.quitadas.length) partes.push(`-${cambio.quitadas.join(', ')}`);
      return h('div', { class: 'cambio-fila' }, icono('tag', 's'), 'Etiquetas', h('span', { class: 'a', text: partes.join(' ') }));
    }
  }
}

/**
 * Nota que se está editando en sitio. Vive fuera del DOM porque la vista se repinta
 * entera con cada evento del backend (vigía de la BD, seguimiento) y el editor se perdía.
 */
let notaEnEdicion: { id: number; texto: string } | null = null;
let enfocarEdicion = false;

function repintar(): void {
  window.dispatchEvent(new Event('migas:repintar'));
}

function pintarNotas(detalle: DetalleEntrada): HTMLElement {
  if (notaEnEdicion && !detalle.notas.some((nota) => nota.id === notaEnEdicion?.id)) {
    notaEnEdicion = null;
  }
  const sec = h('section', { class: 'sec' });
  sec.appendChild(h('div', { class: 'sec-titulo' }, 'Notas ', h('span', { class: 'n', text: String(detalle.notas.length) })));
  const lista = h('div', { class: 'notas' });
  for (const nota of detalle.notas) lista.appendChild(pintarNota(nota));
  sec.appendChild(lista);
  sec.appendChild(pintarCompositor(detalle.entrada.id));
  return sec;
}

function pintarNota(nota: Nota): HTMLElement {
  if (notaEnEdicion?.id === nota.id) return pintarEditorNota(nota, notaEnEdicion);
  const elemento = h('div', { class: `nota${nota.tipo === 'cita' ? ' cita' : ''}` });
  elemento.appendChild(h('div', { class: 'nota-texto', text: nota.texto }));
  const meta = h('div', { class: 'nota-meta' });
  if (nota.tipo === 'cita') meta.appendChild(icono('text-quote', 's'));
  meta.appendChild(h('span', { text: `${nota.tipo === 'cita' ? 'Selección · ' : ''}${diaHora(nota.creada)}${nota.editada ? ' · editada' : ''}` }));
  const acciones = h('div', { class: 'nota-acciones' });
  acciones.appendChild(
    h(
      'button',
      {
        class: 'nota-boton',
        title: 'Editar nota',
        'aria-label': 'Editar nota',
        onclick: () => {
          notaEnEdicion = { id: nota.id, texto: nota.texto };
          enfocarEdicion = true;
          repintar();
        },
      },
      icono('pencil', 's'),
      'Editar',
    ),
  );
  acciones.appendChild(
    h(
      'button',
      {
        class: 'nota-boton peligro',
        title: 'Borrar nota',
        'aria-label': 'Borrar nota',
        onclick: () => void borrarNota(nota.id),
      },
      icono('trash-2', 's'),
      'Borrar',
    ),
  );
  meta.appendChild(acciones);
  elemento.appendChild(meta);
  return elemento;
}

function pintarEditorNota(nota: Nota, edicion: { id: number; texto: string }): HTMLElement {
  const cancelar = (): void => {
    notaEnEdicion = null;
    repintar();
  };
  const guardar = async (): Promise<void> => {
    const texto = area.value.trim();
    if (!texto) return;
    if (texto === nota.texto) {
      cancelar();
      return;
    }
    const previa = notaEnEdicion;
    notaEnEdicion = null;
    if (!(await editarNota(nota.id, texto))) {
      notaEnEdicion = previa;
      repintar();
    }
  };
  const area = h('textarea', {
    class: 'campo',
    'data-foco': `nota-${nota.id}`,
    'aria-label': 'Texto de la nota',
    value: edicion.texto,
    oninput: () => {
      if (notaEnEdicion?.id === nota.id) notaEnEdicion.texto = area.value;
    },
    onkeydown: (evento: KeyboardEvent) => {
      if (evento.key === 'Enter' && (evento.ctrlKey || evento.metaKey)) {
        evento.preventDefault();
        void guardar();
      } else if (evento.key === 'Escape') {
        evento.preventDefault();
        evento.stopPropagation();
        cancelar();
      }
    },
  }) as HTMLTextAreaElement;
  const pie = h('div', { class: 'compositor-pie' });
  pie.appendChild(h('span', { text: 'Ctrl ↵ guardar · Esc cancelar' }));
  pie.appendChild(h('button', { class: 'btn peq', text: 'Cancelar', onclick: cancelar }));
  pie.appendChild(h('button', { class: 'btn peq primario', text: 'Guardar', onclick: () => void guardar() }));
  if (enfocarEdicion) {
    enfocarEdicion = false;
    queueMicrotask(() => {
      area.focus();
      area.setSelectionRange(area.value.length, area.value.length);
    });
  }
  return h('div', { class: `nota editando${nota.tipo === 'cita' ? ' cita' : ''}` }, area, pie);
}

function pintarCompositor(entradaId: number): HTMLElement {
  const caja = h('div', { class: 'compositor' });
  const area = h('textarea', {
    placeholder: 'Escribe una nota…',
    value: borradorDeNota(entradaId),
    oninput: () => guardarBorradorDeNota(entradaId, area.value),
    onkeydown: (evento: KeyboardEvent) => {
      if (evento.key === 'Enter' && (evento.ctrlKey || evento.metaKey)) {
        evento.preventDefault();
        void guardar();
      } else if (evento.key === 'Escape') {
        evento.stopPropagation();
        area.blur();
      }
    },
  }) as HTMLTextAreaElement;
  const guardar = async (): Promise<void> => {
    const texto = area.value.trim();
    if (!texto) return;
    const guardada = await crearNota(entradaId, texto);
    if (!guardada) return;
    eliminarBorradorDeNota(entradaId);
    area.value = '';
    window.dispatchEvent(new Event('migas:repintar'));
  };
  const pie = h('div', { class: 'compositor-pie' });
  pie.appendChild(h('span', { text: 'Ctrl ↵ para guardar' }));
  pie.appendChild(h('button', { class: 'btn primario peq', text: 'Guardar', onclick: () => void guardar() }));
  caja.appendChild(area);
  caja.appendChild(pie);
  return caja;
}

function pintarRelacionadas(entradaId: number, detalle: DetalleEntrada): HTMLElement {
  const sec = h('section', { class: 'sec' });
  const titulo = h('div', { class: 'sec-titulo' }, 'Relacionadas ', h('span', { class: 'n', text: String(detalle.relacionadas.length) }));
  titulo.appendChild(
    h(
      'button',
      {
        class: 'btn fantasma peq',
        onclick: () => window.dispatchEvent(new CustomEvent('migas:paleta-enlazar', { detail: entradaId })),
      },
      icono('link', 's'),
      'Enlazar…',
    ),
  );
  sec.appendChild(titulo);
  const rels = h('div', { class: 'rels' });
  for (const relacion of detalle.relacionadas) rels.appendChild(pintarRelacion(entradaId, relacion));
  sec.appendChild(rels);
  return sec;
}

function pintarRelacion(entradaId: number, relacion: Relacion): HTMLElement {
  const entrada = relacion.entrada;
  const enlace = h(
    'a',
    {
      class: 'rel',
      href: '#',
      'data-fuente': entrada.fuente,
      onclick: (evento: Event) => {
        evento.preventDefault();
        void saltarA(entrada.id, entrada.dia);
      },
    },
    h('span', { class: 'bola' }),
    h('span', { class: 'rel-titulo', text: entrada.titulo }),
  );
  const dia = h('span', { class: 'rel-dia' }, h('span', { text: diaHora(entrada.creada) }));
  dia.appendChild(
    h(
      'button',
      {
        class: 'accion descartar',
        title: 'No está relacionada',
        'aria-label': 'Descartar relación',
        onclick: (evento: Event) => {
          evento.preventDefault();
          evento.stopPropagation();
          void descartar(entradaId, entrada.id);
        },
      },
      icono('x', 's'),
    ),
  );
  enlace.appendChild(dia);
  const motivos = h('span', { class: 'rel-motivos' });
  for (const motivo of relacion.motivos) {
    motivos.appendChild(
      h('span', { class: `motivo${esMotivoFuerte(motivo) ? ' fuerte' : ''}`, text: motivo }),
    );
  }
  enlace.appendChild(motivos);
  return enlace;
}

function esMotivoFuerte(motivo: string): boolean {
  return /^(PROJ-|FS#|WI |!|#)/.test(motivo) || motivo.includes('Enlazada');
}

function pintarHistorial(detalle: DetalleEntrada): HTMLElement {
  const sec = h('section', { class: 'sec' });
  sec.appendChild(h('div', { class: 'sec-titulo', text: 'También guardada' }));
  const historial = h('div', { class: 'historial' });
  for (const aparicion of detalle.historial) {
    historial.appendChild(
      h('button', {
        class: 'chip',
        text: `${etiquetaDia(aparicion.dia)} · ${aparicion.hora}`,
        onclick: () => void saltarA(aparicion.id, aparicion.dia),
      }),
    );
  }
  sec.appendChild(historial);
  return sec;
}

function pintarPie(entradaId: number, recurso: RecursoVista | null): HTMLElement {
  const pie = h('footer', { class: 'det-pie' });
  pie.appendChild(icono('radar', 's'));
  if (!recurso) {
    pie.appendChild(h('span', { text: 'Sin seguimiento' }));
    return pie;
  }
  pie.appendChild(
    h('span', {
      text: recurso.error ? recurso.error : recurso.comprobado ? `Comprobado ${hace(recurso.comprobado)}` : 'Sin comprobar',
    }),
  );
  const etiqueta = h('label', { class: 'interruptor' });
  const casilla = h('input', { type: 'checkbox', checked: recurso.seguir }) as HTMLInputElement;
  casilla.addEventListener('change', () => void seguir(entradaId, casilla.checked));
  etiqueta.appendChild(casilla);
  etiqueta.appendChild(document.createTextNode('Seguir cambios'));
  pie.appendChild(etiqueta);
  return pie;
}
