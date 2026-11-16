/* Aviso de migas dentro de un iframe de la extensión. El estado y la nota van al SW. */

'use strict';

const DURACION = 6000;
const SVG_NS = 'http://www.w3.org/2000/svg';
const movimientoReducido = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const raiz = document.createElement('div');
raiz.className = 'migas-aviso';
raiz.dataset.fuente = 'web';
raiz.dataset.estado = 'cargando';
const icono = document.createElement('div');
icono.className = 'ico';
const txt = document.createElement('div');
txt.className = 'txt';
const t1 = document.createElement('div');
t1.className = 't1';
const t2 = document.createElement('div');
t2.className = 't2';
txt.append(t1, t2);
const cerrar = document.createElement('button');
cerrar.className = 'cerrar';
cerrar.type = 'button';
cerrar.setAttribute('aria-label', 'Cerrar');
cerrar.textContent = '×';
const form = document.createElement('form');
form.className = 'nota';
const input = document.createElement('input');
input.id = 'nota';
input.type = 'text';
input.maxLength = 4000;
input.placeholder = 'Añade una nota y pulsa Enter';
form.appendChild(input);
const barra = document.createElement('div');
barra.className = 'barra';
raiz.append(icono, txt, cerrar, form, barra);
document.body.appendChild(raiz);

let enviando = false;
let cerrado = false;
let pausado = false;
let restante = DURACION;
let inicio = 0;
let temporizador = null;

function iconoSvg(nombre) {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  const caminos = {
    check: [['path', { d: 'M20 6 9 17l-5-5' }]],
    copy: [
      ['rect', { width: '14', height: '14', x: '8', y: '8', rx: '2', ry: '2' }],
      [
        'path',
        {
          d: 'M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2',
        },
      ],
    ],
    x: [
      ['path', { d: 'M18 6 6 18' }],
      ['path', { d: 'm6 6 12 12' }],
    ],
  };
  for (const [tipo, atributos] of caminos[nombre] || caminos.x) {
    const nodo = document.createElementNS(SVG_NS, tipo);
    for (const [clave, valor] of Object.entries(atributos)) nodo.setAttribute(clave, valor);
    svg.appendChild(nodo);
  }
  return svg;
}

function pintarT2(datos) {
  t2.replaceChildren();
  if (datos.estado === 'error') {
    t2.textContent =
      datos.error === 'host'
        ? 'Abre migas → Ajustes → Conexión'
        : datos.error || 'Inténtalo otra vez';
    return;
  }
  const piezas = [];
  if (datos.clave) {
    const span = document.createElement('span');
    span.className = 'clave';
    span.textContent = datos.clave;
    piezas.push(span);
  }
  if (Number(datos.relacionadas) > 0) {
    piezas.push(`${Number(datos.relacionadas)} relacionadas`);
  }
  if (Number(datos.cambios) > 0) {
    const span = document.createElement('span');
    span.className = 'cambios';
    span.textContent = `${Number(datos.cambios)} cambios`;
    piezas.push(span);
  }
  if (piezas.length === 0) {
    t2.textContent = datos.host || 'migas';
    return;
  }
  piezas.forEach((pieza, indice) => {
    if (indice > 0) t2.append(' · ');
    t2.append(pieza);
  });
}

function t1DeEstado(datos) {
  if (datos.estado === 'cargando') return 'Guardando…';
  if (datos.estado === 'error') return 'No se pudo guardar';
  if (datos.estado === 'dup') {
    return datos.duplicada && String(datos.seleccion || '').trim()
      ? 'Selección añadida'
      : 'Ya estaba en el diario de hoy';
  }
  return 'Guardada en migas';
}

function programar(ms) {
  if (temporizador !== null) clearTimeout(temporizador);
  inicio = Date.now();
  temporizador = window.setTimeout(() => {
    temporizador = null;
    cerrarAviso();
  }, Math.max(0, ms));
}

function debePausar() {
  if (input.value.trim() !== '') return true;
  if (raiz.matches(':hover')) return true;
  return Boolean(
    document.hasFocus() &&
      (document.activeElement === input || raiz.contains(document.activeElement)),
  );
}

function pausar() {
  if (temporizador === null || pausado || cerrado) return;
  restante = Math.max(0, restante - (Date.now() - inicio));
  clearTimeout(temporizador);
  temporizador = null;
  pausado = true;
  barra.style.animationPlayState = 'paused';
}

function reanudar() {
  if (!pausado || cerrado) return;
  pausado = false;
  barra.style.animationPlayState = 'running';
  programar(restante);
}

function actualizarPausa() {
  const debe = debePausar();
  if (debe && !pausado) pausar();
  else if (!debe && pausado) reanudar();
}

function iniciarCuenta() {
  if (temporizador !== null) clearTimeout(temporizador);
  temporizador = null;
  pausado = false;
  restante = DURACION;
  if (!movimientoReducido) {
    barra.style.animation = 'none';
    void barra.offsetWidth;
    barra.style.animation = '';
  }
  programar(DURACION);
  actualizarPausa();
}

function cerrarAviso() {
  if (cerrado) return;
  cerrado = true;
  if (temporizador !== null) clearTimeout(temporizador);
  temporizador = null;
  raiz.classList.add('sale');
  try {
    chrome.runtime.sendMessage({ tipo: 'cerrarAviso' }, () => {
      void chrome.runtime.lastError;
    });
  } catch {
    // Sin extensión.
  }
  window.setTimeout(() => {
    raiz.style.display = 'none';
  }, 230);
}

function aplicar(datos) {
  cerrado = false;
  raiz.style.display = '';
  raiz.classList.remove('sale');
  raiz.dataset.estado = datos.estado || 'cargando';
  raiz.dataset.fuente = datos.fuente || 'web';
  icono.replaceChildren(
    iconoSvg(
      datos.estado === 'ok' ? 'check' : datos.estado === 'error' ? 'x' : 'copy',
    ),
  );
  t1.textContent = t1DeEstado(datos);
  pintarT2(datos);
  if (datos.estado === 'cargando') {
    if (temporizador !== null) clearTimeout(temporizador);
    temporizador = null;
    pausado = false;
    input.value = '';
    input.disabled = true;
    input.placeholder = 'Guardando…';
    barra.style.animation = 'none';
    return;
  }
  input.disabled = false;
  input.placeholder = 'Añade una nota y pulsa Enter';
  input.focus({ preventScroll: true });
  iniciarCuenta();
}

function enviarNota() {
  if (enviando || cerrado) return;
  const texto = input.value.trim();
  if (!texto) {
    input.focus();
    return;
  }
  enviando = true;
  input.disabled = true;
  try {
    chrome.runtime.sendMessage({ tipo: 'notaAviso', texto }, (respuesta) => {
      const error = chrome.runtime.lastError;
      if (cerrado) return;
      enviando = false;
      input.disabled = false;
      if (error || !respuesta || !respuesta.ok) {
        t2.textContent =
          respuesta && respuesta.error
            ? String(respuesta.error)
            : 'No se pudo añadir la nota';
        actualizarPausa();
        input.focus();
        return;
      }
      input.value = '';
      form.classList.add('enviada');
      if (temporizador !== null) clearTimeout(temporizador);
      temporizador = window.setTimeout(cerrarAviso, 700);
    });
  } catch (error) {
    enviando = false;
    input.disabled = false;
    t2.textContent = String(error);
    actualizarPausa();
  }
}

cerrar.addEventListener('click', cerrarAviso);
form.addEventListener('submit', (evento) => {
  evento.preventDefault();
  enviarNota();
});
input.addEventListener('input', actualizarPausa);
raiz.addEventListener('mouseenter', actualizarPausa);
raiz.addEventListener('mouseleave', actualizarPausa);
raiz.addEventListener('focusin', actualizarPausa);
raiz.addEventListener('focusout', actualizarPausa);
window.addEventListener(
  'keydown',
  (evento) => {
    if (evento.key !== 'Escape') return;
    evento.preventDefault();
    evento.stopPropagation();
    cerrarAviso();
  },
  true,
);
window.addEventListener('focus', () => {
  input.focus({ preventScroll: true });
  actualizarPausa();
});

chrome.runtime.sendMessage({ tipo: 'estadoAviso' }, (respuesta) => {
  const error = chrome.runtime.lastError;
  if (error || !respuesta || !respuesta.ok) {
    t1.textContent = 'No se pudo cargar el aviso';
    t2.textContent = 'Cierra y vuelve a intentarlo';
    return;
  }
  aplicar(respuesta);
});
