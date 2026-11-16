/* Popup de migas: estado de la pestaña, nota y acciones contra el host nativo. */

'use strict';

const titulo = document.getElementById('titulo');
const resumen = document.getElementById('resumen');
const nota = document.getElementById('nota');
const guardar = document.getElementById('guardar');
const abrir = document.getElementById('abrir');
const ayuda = document.getElementById('ayuda');
const error = document.getElementById('error');

let pestana = null;
let guardada = false;
let idGuardada = null;

function pestanaActiva() {
  return new Promise((resolver) => {
    chrome.tabs.query({ active: true, currentWindow: true }, (pestanas) => {
      resolver((pestanas && pestanas[0]) || null);
    });
  });
}

function mensaje(datos) {
  return new Promise((resolver) => {
    try {
      chrome.runtime.sendMessage(datos, (respuesta) => {
        const ultimoError = chrome.runtime.lastError;
        if (ultimoError) {
          const texto = String(ultimoError.message || ultimoError);
          resolver({
            ok: false,
            error: /native|host|not found|not registered/i.test(texto) ? 'host' : texto,
          });
          return;
        }
        resolver(respuesta || { ok: false, error: 'Sin respuesta' });
      });
    } catch (excepcion) {
      resolver({ ok: false, error: String(excepcion) });
    }
  });
}

function mostrarError(texto, esHost = false) {
  error.hidden = !texto;
  error.textContent = texto || '';
  ayuda.hidden = !esHost;
}

function pintarResumen(respuesta) {
  const partes = [respuesta.guardadaHoy ? 'Guardada hoy' : 'No guardada'];
  if (respuesta.guardadaHoy) {
    if (respuesta.cambios > 0) partes.push(`${respuesta.cambios} cambios`);
    if (respuesta.relacionadas > 0) partes.push(`${respuesta.relacionadas} relacionadas`);
    if (respuesta.veces > 1) partes.push(`${respuesta.veces} días`);
  }
  resumen.textContent = partes.join(' · ');
  guardar.textContent = respuesta.guardadaHoy ? 'Añadir nota' : 'Guardar página';
}

async function cargar() {
  pestana = await pestanaActiva();
  if (!pestana) {
    titulo.textContent = 'Sin pestaña';
    resumen.textContent = 'Abre una página para guardarla';
    guardar.disabled = true;
    return;
  }
  titulo.textContent = pestana.title || pestana.url || 'Esta página';
  if (!/^https?:\/\//i.test(pestana.url || '')) {
    resumen.textContent = 'Esta página no se puede guardar';
    guardar.disabled = true;
    mostrarError('Abre una página web normal (http o https).');
    return;
  }
  guardar.disabled = true;
  mostrarError('');
  guardada = false;
  idGuardada = null;
  const respuesta = await mensaje({ tipo: 'estado', url: pestana.url });
  guardar.disabled = false;
  if (!respuesta.ok) {
    if (respuesta.error === 'host') {
      resumen.textContent = 'No guardada';
      mostrarError('El host de migas no responde.', true);
    } else {
      resumen.textContent = 'No guardada';
      mostrarError(respuesta.error || 'No se pudo consultar el estado.');
    }
    return;
  }
  guardada = Boolean(respuesta.guardadaHoy);
  idGuardada = typeof respuesta.id === 'number' ? respuesta.id : null;
  pintarResumen(respuesta);
}

async function alGuardar() {
  if (!pestana) return;
  const texto = nota.value.trim();
  if (guardada && idGuardada !== null) {
    if (!texto) {
      nota.focus();
      mostrarError('Escribe una nota para añadirla.');
      return;
    }
    guardar.disabled = true;
    mostrarError('');
    const respuesta = await mensaje({ tipo: 'nota', id: idGuardada, texto });
    guardar.disabled = false;
    if (!respuesta.ok) {
      mostrarError(
        respuesta.error === 'host' ? 'El host de migas no responde.' : respuesta.error,
        respuesta.error === 'host',
      );
      return;
    }
    nota.value = '';
    await cargar();
    resumen.textContent = 'Nota añadida';
    return;
  }

  guardar.disabled = true;
  mostrarError('');
  const respuesta = await mensaje({
    tipo: 'capturar',
    url: pestana.url,
    titulo: pestana.title || '',
    seleccion: '',
    nota: texto,
  });
  guardar.disabled = false;
  if (!respuesta.ok) {
    mostrarError(
      respuesta.error === 'host' ? 'El host de migas no responde.' : respuesta.error,
      respuesta.error === 'host',
    );
    return;
  }
  guardada = true;
  idGuardada = typeof respuesta.id === 'number' ? respuesta.id : null;
  nota.value = '';
  await cargar();
  resumen.textContent = respuesta.duplicada ? 'Ya estaba en el diario de hoy' : 'Guardada';
  guardar.textContent = 'Añadir nota';
}

guardar.addEventListener('click', () => {
  void alGuardar();
});

abrir.addEventListener('click', (evento) => {
  evento.preventDefault();
  void mensaje({ tipo: 'abrir', id: idGuardada }).then(() => window.close());
});

void cargar();
