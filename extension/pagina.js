/* Content script de migas: detector de doble Ctrl, host del aviso en iframe y visitas. */

(function () {
  'use strict';

  const ANTERIOR = globalThis.__migasPagina;
  if (ANTERIOR && typeof ANTERIOR.desactivar === 'function') {
    try {
      ANTERIOR.desactivar();
    } catch {
      // Contexto antiguo ya inutilizado.
    }
  }
  if (!chrome.runtime?.id) return;

  /** Jira, GitLab o Freshservice: mismo criterio que `fuenteDeUrl` de fondo.js. */
  function esPaginaSeguida() {
    const host = location.hostname;
    const ruta = location.pathname;
    return (
      host.endsWith('atlassian.net') ||
      ruta.startsWith('/browse/') ||
      host.endsWith('freshservice.com') ||
      host.split('.').includes('gitlab') ||
      ruta.includes('/-/')
    );
  }
  const limpiezas = [];
  const detectorApi = globalThis.migasDetector;
  const detector =
    detectorApi && typeof detectorApi.crearDetector === 'function'
      ? detectorApi.crearDetector({ ms: 330, ahora: () => performance.now() })
      : null;

  let avisoHost = null;
  let avisoShadow = null;
  let avisoIframe = null;
  let focoAnterior = null;
  let capturando = false;

  const estado = {
    activo: true,
    sigueVivo: () => {
      try {
        return Boolean(chrome.runtime?.id);
      } catch {
        return false;
      }
    },
    desactivar: () => {
      if (!estado.activo) return;
      estado.activo = false;
      for (const limpiar of limpiezas.splice(0)) {
        try {
          limpiar();
        } catch {
          // Contexto invalidado.
        }
      }
      if (avisoHost) {
        avisoHost.remove();
        avisoHost = null;
        avisoShadow = null;
        avisoIframe = null;
        focoAnterior = null;
      }
    },
  };
  globalThis.__migasPagina = estado;

  function vivo() {
    if (!estado.activo) return false;
    if (!estado.sigueVivo()) {
      estado.desactivar();
      return false;
    }
    return true;
  }

  function estaEnAviso(evento) {
    if (!avisoHost) return false;
    const camino = typeof evento.composedPath === 'function' ? evento.composedPath() : [];
    if (camino.includes(avisoHost)) return true;
    return evento.target === avisoHost || avisoHost.contains(evento.target);
  }

  function avisoEnfocado() {
    if (!avisoHost) return false;
    if (document.activeElement === avisoHost || document.activeElement === avisoIframe) {
      return true;
    }
    return Boolean(avisoShadow && avisoShadow.activeElement === avisoIframe);
  }

  function crearAviso() {
    const host = document.createElement('div');
    host.style.cssText =
      'position:fixed;right:0;bottom:0;width:360px;height:170px;border:0;background:transparent;z-index:2147483647;';
    const shadow = host.attachShadow({ mode: 'closed' });
    const iframe = document.createElement('iframe');
    iframe.src = chrome.runtime.getURL('aviso.html');
    iframe.title = 'Aviso de migas';
    iframe.setAttribute('aria-label', 'Aviso de migas');
    iframe.style.cssText =
      'display:block;width:100%;height:100%;border:0;background:transparent;color-scheme:dark;';
    iframe.addEventListener('load', () => iframe.focus());
    shadow.appendChild(iframe);
    document.documentElement.appendChild(host);
    avisoHost = host;
    avisoShadow = shadow;
    avisoIframe = iframe;
  }

  function abrirAviso() {
    if (!vivo()) return;
    if (!avisoHost) {
      focoAnterior =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
    } else {
      avisoHost.remove();
      avisoHost = null;
      avisoShadow = null;
      avisoIframe = null;
    }
    crearAviso();
    avisoIframe.focus();
  }

  function quitarAviso() {
    if (!avisoHost) return;
    const host = avisoHost;
    const foco = focoAnterior;
    avisoHost = null;
    avisoShadow = null;
    avisoIframe = null;
    focoAnterior = null;
    host.remove();
    if (foco && foco.isConnected) foco.focus({ preventScroll: true });
  }

  function leerSeleccion() {
    try {
      return window.getSelection() ? window.getSelection().toString() : '';
    } catch {
      return '';
    }
  }

  function enviarMensaje(mensaje) {
    return new Promise((resolver) => {
      if (!vivo()) {
        resolver({ ok: false, error: 'host' });
        return;
      }
      try {
        chrome.runtime.sendMessage(mensaje, (respuesta) => {
          const error = chrome.runtime.lastError;
          if (error) {
            resolver({ ok: false, error: 'host' });
            return;
          }
          resolver(respuesta || { ok: false, error: 'Sin respuesta' });
        });
      } catch {
        resolver({ ok: false, error: 'host' });
      }
    });
  }

  async function capturar() {
    if (!vivo() || capturando) return;
    capturando = true;
    const seleccion = leerSeleccion();
    await enviarMensaje({
      tipo: 'capturar',
      url: location.href,
      titulo: document.title,
      seleccion,
    });
    capturando = false;
  }

  // ── Detector de doble Ctrl ────────────────────────────────────────────────

  function configurarDetector() {
    if (!detector) return;
    try {
      chrome.storage.local.get({ dobleCtrlMs: 330 }, (valores) => {
        if (!vivo()) return;
        if (valores && Number.isFinite(Number(valores.dobleCtrlMs))) {
          detector.setMs(Number(valores.dobleCtrlMs));
        }
      });
      const alCambiarStorage = (cambios, area) => {
        if (!vivo() || area !== 'local' || !cambios.dobleCtrlMs) return;
        const valor = Number(cambios.dobleCtrlMs.newValue);
        if (Number.isFinite(valor) && valor > 0) detector.setMs(valor);
      };
      chrome.storage.onChanged.addListener(alCambiarStorage);
      limpiezas.push(() => chrome.storage.onChanged.removeListener(alCambiarStorage));
    } catch {
      // Sin storage (contexto invalidado): se queda con 330 ms.
    }
  }

  const alKeydown = (evento) => {
    if (!vivo() || !detector || !evento.isTrusted) return;
    if (evento.key === 'Escape' && avisoHost) {
      evento.preventDefault();
      quitarAviso();
      return;
    }
    if (evento.key === 'Control') {
      if (evento.repeat || avisoEnfocado()) return;
      detector.keydown(evento);
      return;
    }
    if (estaEnAviso(evento)) return;
    detector.otra();
  };

  const alKeyup = (evento) => {
    if (!vivo() || !detector || !evento.isTrusted) return;
    if (evento.key === 'Control') {
      if (avisoEnfocado()) return;
      if (detector.keyup(evento)) void capturar();
      return;
    }
    if (!estaEnAviso(evento)) detector.otra();
  };

  const alPointer = (evento) => {
    if (!vivo() || !detector || !evento.isTrusted || estaEnAviso(evento)) return;
    detector.otra();
  };

  const alWheel = (evento) => {
    if (!vivo() || !detector || !evento.isTrusted || estaEnAviso(evento)) return;
    detector.rueda();
  };

  window.addEventListener('keydown', alKeydown, true);
  window.addEventListener('keyup', alKeyup, true);
  window.addEventListener('pointerdown', alPointer, true);
  window.addEventListener('wheel', alWheel, { capture: true, passive: true });
  limpiezas.push(
    () => window.removeEventListener('keydown', alKeydown, true),
    () => window.removeEventListener('keyup', alKeyup, true),
    () => window.removeEventListener('pointerdown', alPointer, true),
    () => window.removeEventListener('wheel', alWheel, true),
  );

  const alMensaje = (mensaje) => {
    if (!vivo() || !mensaje || typeof mensaje.tipo !== 'string') return;
    if (mensaje.tipo === 'abrirAviso') abrirAviso();
    else if (mensaje.tipo === 'quitarAviso') quitarAviso();
  };
  chrome.runtime.onMessage.addListener(alMensaje);
  limpiezas.push(() => chrome.runtime.onMessage.removeListener(alMensaje));

  // ── Visitas a recursos seguidos ───────────────────────────────────────────

  const visitadas = new Map();

  function enviarVisita(url) {
    if (!vivo()) return;
    try {
      chrome.runtime.sendMessage({ tipo: 'visita', url }, () => {
        void chrome.runtime.lastError;
      });
    } catch {
      // Extensión recargada: se ignora.
    }
  }

  function visitar(url) {
    if (!vivo() || !esPaginaSeguida()) return;
    const ahora = Date.now();
    const anterior = visitadas.get(url);
    if (anterior && ahora - anterior < 30000) return;
    visitadas.set(url, ahora);
    enviarVisita(url);
  }

  function vigilarNavegacion() {
    if (!esPaginaSeguida()) return;
    visitar(location.href);
    let ultimaUrl = location.href;
    const alPop = () => visitar(location.href);
    window.addEventListener('popstate', alPop);
    const intervalo = setInterval(() => {
      if (!vivo() || location.href === ultimaUrl) return;
      ultimaUrl = location.href;
      visitar(location.href);
    }, 1500);
    limpiezas.push(
      () => window.removeEventListener('popstate', alPop),
      () => clearInterval(intervalo),
    );
  }

  configurarDetector();
  vigilarNavegacion();
})();
