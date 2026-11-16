/* Service worker de migas. Habla con el host nativo `es.migas.host`, guarda el estado
 * del aviso por pestaña y responde a la página y al popup. Sin dependencias. */

'use strict';

const HOST = 'es.migas.host';
const AVISOS = new Map();
const PREFIJO_AVISO = 'aviso_';

/** Jira, GitLab o Freshservice: se reconocen por el host o por la forma de la ruta. */
function esHostSeguido(url) {
  return fuenteDeUrl(url) !== 'web';
}

function hostDe(url) {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
}

function fuenteDeUrl(url) {
  try {
    const { hostname: host, pathname: ruta } = new URL(url);
    if (host.endsWith('atlassian.net') || ruta.startsWith('/browse/')) return 'jira';
    if (host.endsWith('freshservice.com')) return 'fs';
    if (host.split('.').includes('gitlab') || ruta.includes('/-/')) return 'gitlab';
  } catch {
    // URL no válida.
  }
  return 'web';
}

function esErrorDeHost(texto) {
  return /not found|not registered|specified native messaging host|no such native/i.test(
    String(texto || ''),
  );
}

/** Envía una petición al host y normaliza la respuesta. */
function enviar(mensaje) {
  return new Promise((resolver) => {
    const peticion = { v: 1, ...mensaje };
    try {
      chrome.runtime.sendNativeMessage(HOST, peticion, (respuesta) => {
        const error = chrome.runtime.lastError;
        if (error) {
          const texto = String(error.message || error);
          resolver({ ok: false, error: esErrorDeHost(texto) ? 'host' : texto });
          return;
        }
        if (!respuesta || typeof respuesta !== 'object') {
          resolver({ ok: false, error: 'Sin respuesta del host' });
          return;
        }
        resolver(respuesta);
      });
    } catch (error) {
      resolver({ ok: false, error: esErrorDeHost(error) ? 'host' : String(error) });
    }
  });
}

function pestanaActiva() {
  return new Promise((resolver) => {
    chrome.tabs.query({ active: true, currentWindow: true }, (pestanas) => {
      resolver((pestanas && pestanas[0]) || null);
    });
  });
}

async function urlDePestana(mensaje, remitente) {
  if (mensaje && typeof mensaje.url === 'string' && mensaje.url) return mensaje.url;
  if (remitente && remitente.tab && remitente.tab.url) return remitente.tab.url;
  const pestana = await pestanaActiva();
  return (pestana && pestana.url) || '';
}

// ── Estado del aviso por pestaña (memoria + storage.session) ────────────────

function claveAviso(tabId) {
  return `${PREFIJO_AVISO}${tabId}`;
}

async function guardarAviso(tabId, estado) {
  if (!Number.isInteger(tabId)) return;
  AVISOS.set(tabId, estado);
  try {
    await chrome.storage.session.set({ [claveAviso(tabId)]: estado });
  } catch {
    // Sin storage.session: queda la copia en memoria.
  }
}

async function leerAviso(tabId) {
  if (AVISOS.has(tabId)) return AVISOS.get(tabId);
  try {
    const datos = await chrome.storage.session.get(claveAviso(tabId));
    return datos[claveAviso(tabId)] || null;
  } catch {
    return null;
  }
}

async function borrarAviso(tabId) {
  AVISOS.delete(tabId);
  try {
    await chrome.storage.session.remove(claveAviso(tabId));
  } catch {
    // Sin storage.session.
  }
}

function estadoCargando(tab, seleccion = '') {
  const url = tab && tab.url ? tab.url : '';
  return {
    estado: 'cargando',
    id: null,
    fuente: fuenteDeUrl(url),
    titulo: (tab && tab.title) || '',
    host: hostDe(url),
    clave: null,
    cambios: 0,
    relacionadas: 0,
    error: null,
    duplicada: false,
    seleccion,
  };
}

function estadoResultado(resultado, tab, seleccion = '') {
  const url = tab && tab.url ? tab.url : '';
  return {
    estado: resultado.ok ? (resultado.duplicada ? 'dup' : 'ok') : 'error',
    id: typeof resultado.id === 'number' ? resultado.id : null,
    fuente: resultado.fuente || fuenteDeUrl(url),
    titulo: resultado.titulo || (tab && tab.title) || '',
    host: hostDe(url),
    clave: resultado.clave || null,
    cambios: Number(resultado.cambios) || 0,
    relacionadas: Number(resultado.relacionadas) || 0,
    error: resultado.error || null,
    duplicada: Boolean(resultado.duplicada),
    seleccion,
  };
}

function abrirAvisoEnPestana(tabId) {
  if (!tabId) return;
  try {
    chrome.tabs.sendMessage(tabId, { tipo: 'abrirAviso' }, { frameId: 0 }, () => {
      // Sin content script (chrome://, Web Store): se ignora.
      void chrome.runtime.lastError;
    });
  } catch {
    // Sin content script.
  }
}

function quitarAvisoEnPestana(tabId) {
  if (!tabId) return;
  try {
    chrome.tabs.sendMessage(tabId, { tipo: 'quitarAviso' }, { frameId: 0 }, () => {
      void chrome.runtime.lastError;
    });
  } catch {
    // Sin content script.
  }
}

async function publicarAviso(tabId, estado) {
  await guardarAviso(tabId, estado);
  abrirAvisoEnPestana(tabId);
}

// ── Validación de remitentes ────────────────────────────────────────────────

function esAviso(remitente) {
  const prefijo = chrome.runtime.getURL('aviso.html');
  return Boolean(
    remitente &&
      remitente.tab &&
      Number.isInteger(remitente.tab.id) &&
      typeof remitente.url === 'string' &&
      remitente.url.startsWith(prefijo),
  );
}

function esContenido(remitente) {
  return Boolean(
    remitente &&
      remitente.tab &&
      Number.isInteger(remitente.tab.id) &&
      remitente.frameId === 0,
  );
}

// ── Mensajes del aviso ──────────────────────────────────────────────────────

async function responderAviso(tipo, mensaje, remitente) {
  if (!esAviso(remitente)) return { ok: false, error: 'No autorizado' };
  const tabId = remitente.tab.id;
  if (tipo === 'estadoAviso') {
    const estado = await leerAviso(tabId);
    return estado ? { ok: true, ...estado } : { ok: false, error: 'Sin aviso' };
  }
  if (tipo === 'notaAviso') {
    const estado = await leerAviso(tabId);
    if (!estado || typeof estado.id !== 'number') {
      return { ok: false, error: 'No hay una miga guardada' };
    }
    return enviar({
      tipo: 'nota',
      id: estado.id,
      texto: typeof mensaje.texto === 'string' ? mensaje.texto : '',
    });
  }
  if (tipo === 'cerrarAviso') {
    await borrarAviso(tabId);
    quitarAvisoEnPestana(tabId);
    return { ok: true };
  }
  return { ok: false, error: 'Petición no válida' };
}

// ── Capturas ────────────────────────────────────────────────────────────────

async function responderCaptura(mensaje, remitente) {
  let url = mensaje.url;
  let titulo = mensaje.titulo;
  if (!url || !titulo) {
    const pestana = remitente && remitente.tab ? remitente.tab : await pestanaActiva();
    url = url || (pestana && pestana.url) || '';
    titulo = titulo || (pestana && pestana.title) || '';
  }
  const seleccion =
    typeof mensaje.seleccion === 'string' ? mensaje.seleccion : '';
  const tabId = remitente && remitente.tab ? remitente.tab.id : null;
  const pestana = {
    id: tabId,
    url,
    title: titulo,
  };
  if (Number.isInteger(tabId)) {
    await publicarAviso(tabId, estadoCargando(pestana, seleccion));
  }
  const resultado = await enviar({
    tipo: 'capturar',
    url,
    titulo,
    seleccion,
    nota: typeof mensaje.nota === 'string' ? mensaje.nota : '',
  });
  if (Number.isInteger(tabId)) {
    await publicarAviso(tabId, estadoResultado(resultado, pestana, seleccion));
  }
  return resultado;
}

async function responder(mensaje, remitente) {
  const tipo = mensaje && mensaje.tipo;
  if (tipo === 'capturar') {
    if (remitente && remitente.tab && !esContenido(remitente)) {
      return { ok: false, error: 'No autorizado' };
    }
    return responderCaptura(mensaje, remitente);
  }

  if (tipo === 'nota') {
    return enviar({
      tipo: 'nota',
      id: mensaje.id,
      texto: typeof mensaje.texto === 'string' ? mensaje.texto : '',
    });
  }

  if (tipo === 'visita') {
    const url = await urlDePestana(mensaje, remitente);
    if (!url) return { ok: false, error: 'Sin URL' };
    if (!esHostSeguido(url)) return { ok: true, marcada: false };
    return enviar({ tipo: 'visita', url });
  }

  if (tipo === 'estado') {
    const url = await urlDePestana(mensaje, remitente);
    if (!url) return { ok: false, error: 'Sin URL' };
    return enviar({ tipo: 'estado', url });
  }

  if (tipo === 'abrir') {
    return enviar({ tipo: 'abrir', id: typeof mensaje.id === 'number' ? mensaje.id : null });
  }

  return { ok: false, error: 'Petición no válida' };
}

chrome.runtime.onMessage.addListener((mensaje, remitente, responderMensaje) => {
  const tipo = mensaje && mensaje.tipo;
  if (tipo === 'estadoAviso' || tipo === 'notaAviso' || tipo === 'cerrarAviso') {
    responderAviso(tipo, mensaje, remitente)
      .then(responderMensaje)
      .catch((error) => responderMensaje({ ok: false, error: String(error) }));
    return true;
  }
  responder(mensaje, remitente)
    .then(responderMensaje)
    .catch((error) => responderMensaje({ ok: false, error: String(error) }));
  return true;
});

function mostrarBadge(tabId, texto) {
  if (!tabId || !chrome.action) return;
  try {
    chrome.action.setBadgeText({ tabId, text: texto });
    chrome.action.setBadgeBackgroundColor({
      tabId,
      color: texto === '✓' ? '#2f9e44' : '#c92a2a',
    });
    setTimeout(() => {
      chrome.action.setBadgeText({ tabId, text: '' });
    }, 2000);
  } catch {
    // Sin badge disponible.
  }
}

async function capturarPestanaActiva() {
  const pestana = await pestanaActiva();
  if (!pestana || !pestana.id) return;

  await publicarAviso(pestana.id, estadoCargando(pestana));
  let seleccion = '';
  try {
    const resultados = await chrome.scripting.executeScript({
      target: { tabId: pestana.id },
      func: () => (window.getSelection() ? window.getSelection().toString() : ''),
    });
    seleccion = (resultados && resultados[0] && resultados[0].result) || '';
  } catch {
    // Páginas internas: sin content script ni selección.
  }

  const resultado = await enviar({
    tipo: 'capturar',
    url: pestana.url || '',
    titulo: pestana.title || '',
    seleccion,
  });
  await publicarAviso(pestana.id, estadoResultado(resultado, pestana, seleccion));
  mostrarBadge(pestana.id, resultado.ok ? '✓' : '!');
}

chrome.commands.onCommand.addListener((comando) => {
  if (comando === 'capturar') void capturarPestanaActiva();
});

function pestanasTodas() {
  return new Promise((resolver) => {
    chrome.tabs.query({}, (pestanas) => resolver(pestanas || []));
  });
}

async function inyectarEnPestanasExistentes() {
  const pestanas = await pestanasTodas();
  for (const pestana of pestanas) {
    if (!Number.isInteger(pestana.id)) continue;
    if (!/^https?:\/\//i.test(pestana.url || '')) continue;
    try {
      await chrome.scripting.executeScript({
        target: { tabId: pestana.id },
        files: ['detector.js', 'pagina.js'],
      });
    } catch {
      // Pestaña protegida, descartada o sin permiso: se ignora.
    }
  }
}

chrome.runtime.onInstalled.addListener(() => {
  void inyectarEnPestanasExistentes();
});

chrome.runtime.onStartup.addListener(() => {
  void inyectarEnPestanasExistentes();
});
