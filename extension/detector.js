/*
 * Detector puro de la doble pulsación de Ctrl. Sin DOM ni APIs de Chrome: la página le
 * pasa eventos y las pruebas lo cargan con `createRequire`.
 *
 * La doble pulsación se confirma al soltar la segunda (CtrlArriba), igual que el detector
 * Rust de `src-tauri/src/tecla.rs`: cada tap < 250 ms, la segunda empieza < `ms` desde la
 * primera suelta y cualquier otra tecla, clic o rueda durante el gesto lo anula. Una triple
 * pulsación produce una sola detección.
 */
(function (global) {
  'use strict';

  if (global.__migasDetectorCargado && global.migasDetector) {
    if (typeof module !== 'undefined' && module.exports) {
      module.exports = global.migasDetector;
    }
    return;
  }
  global.__migasDetectorCargado = true;

  function crearDetector(opciones = {}) {
    const msInicial = Number.isFinite(opciones.ms) ? opciones.ms : 330;
    const ahora =
      typeof opciones.ahora === 'function'
        ? opciones.ahora
        : () => (global.performance && global.performance.now ? global.performance.now() : Date.now());

    let ms = msInicial;
    let abajo = null;
    let ultimaSuelta = null;
    let candidato = false;
    let bloqueadoHasta = null;
    let segunda = false;
    let cancelado = false;

    const tiempo = (t) => (t === undefined || t === null ? Number(ahora()) : Number(t));
    const limpiar = () => {
      abajo = null;
      ultimaSuelta = null;
      candidato = false;
      bloqueadoHasta = null;
      segunda = false;
      cancelado = false;
    };

    function evento(entrada, t) {
      const momento = tiempo(t);
      const tipo = normalizar(entrada);
      if (tipo === 'CtrlAbajo') {
        if (abajo !== null) return false; // autorepeat
        if (bloqueadoHasta !== null) {
          if (momento <= bloqueadoHasta) {
            abajo = momento;
            segunda = false;
            cancelado = true;
            candidato = false;
            ultimaSuelta = null;
            return false;
          }
          bloqueadoHasta = null;
        }
        const esSegunda =
          candidato &&
          ultimaSuelta !== null &&
          momento - ultimaSuelta < ms;
        abajo = momento;
        segunda = esSegunda;
        cancelado = false;
        candidato = false;
        ultimaSuelta = null;
        return false;
      }

      if (tipo === 'CtrlArriba') {
        if (abajo === null) {
          candidato = false;
          ultimaSuelta = null;
          segunda = false;
          bloqueadoHasta = null;
          return false;
        }
        const duracion = momento - abajo;
        if (bloqueadoHasta !== null) {
          bloqueadoHasta = null;
          segunda = false;
          candidato = false;
          ultimaSuelta = null;
          cancelado = false;
          abajo = null;
          return false;
        }
        if (segunda) {
          segunda = false;
          abajo = null;
          const detecta = !cancelado && duracion < 250;
          cancelado = false;
          candidato = false;
          ultimaSuelta = null;
          if (detecta) bloqueadoHasta = momento + ms;
          return detecta;
        }
        segunda = false;
        abajo = null;
        if (!cancelado && duracion < 250) {
          ultimaSuelta = momento;
          candidato = true;
        } else {
          candidato = false;
          ultimaSuelta = null;
        }
        cancelado = false;
        return false;
      }

      // OtraTecla o Rueda: anula el gesto completo.
      cancelado = true;
      candidato = false;
      ultimaSuelta = null;
      return false;
    }

    return {
      evento,
      keydown(eventoTeclado) {
        if (!eventoTeclado || typeof eventoTeclado.key !== 'string') return false;
        if (eventoTeclado.key === 'Control') {
          if (eventoTeclado.repeat) return false;
          return evento('CtrlAbajo');
        }
        return evento('OtraTecla');
      },
      keyup(eventoTeclado) {
        if (!eventoTeclado || typeof eventoTeclado.key !== 'string') return false;
        return evento(eventoTeclado.key === 'Control' ? 'CtrlArriba' : 'OtraTecla');
      },
      otra() {
        return evento('OtraTecla');
      },
      rueda() {
        return evento('Rueda');
      },
      setMs(nuevo) {
        const valor = Number(nuevo);
        if (Number.isFinite(valor) && valor > 0) ms = valor;
      },
      get ms() {
        return ms;
      },
      reiniciar: limpiar,
    };
  }

  function normalizar(entrada) {
    if (typeof entrada === 'string') {
      if (entrada === 'CtrlAbajo' || entrada === 'CtrlArriba') return entrada;
      if (entrada === 'Rueda') return 'Rueda';
      return 'OtraTecla';
    }
    if (entrada && typeof entrada === 'object') {
      if (entrada.tipo === 'CtrlAbajo' || entrada.tipo === 'CtrlArriba') return entrada.tipo;
      if (entrada.tipo === 'Rueda') return 'Rueda';
      if (entrada.key === 'Control') {
        return entrada.type === 'keyup' ? 'CtrlArriba' : 'CtrlAbajo';
      }
    }
    return 'OtraTecla';
  }

  const api = { crearDetector };
  global.migasDetector = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
