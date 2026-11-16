import {
  abrirEntrada,
  borrar,
  borrarDefinitivo,
  cargarDetalle,
  copiarEntrada,
  diaAnteriorConEntradas,
  diaSiguienteConEntradas,
  entradaSeleccionada,
  fijar,
  get,
  irDia,
  irHoy,
  irVista,
  marcarVisto,
  quitarBusqueda,
  moverSeleccion,
  set,
} from './estado';
import type { Vista } from './types';

export interface AccionesTeclado {
  abrirPaleta: () => void;
  abrirAjustes: () => void;
  abrirRapida: () => void;
}

const VISTAS: Vista[] = ['dia', 'cambios', 'fijadas', 'todo', 'papelera'];

function enEdicion(): boolean {
  const activo = document.activeElement;
  return (
    activo instanceof HTMLInputElement ||
    activo instanceof HTMLTextAreaElement ||
    (activo instanceof HTMLElement && activo.isContentEditable)
  );
}

function hayVelo(): boolean {
  return document.querySelector('.velo') !== null;
}

export function instalarTeclas(acciones: AccionesTeclado): void {
  window.addEventListener('keydown', (evento) => {
    if (evento.defaultPrevented) return;
    const control = evento.ctrlKey || evento.metaKey;
    if (control && !evento.altKey && !evento.shiftKey && evento.key.toLowerCase() === 'k') {
      evento.preventDefault();
      acciones.abrirPaleta();
      return;
    }
    if (control && !evento.altKey && !evento.shiftKey && evento.key === ',') {
      evento.preventDefault();
      acciones.abrirAjustes();
      return;
    }
    if (control && !evento.altKey && !evento.shiftKey && evento.key.toLowerCase() === 'n') {
      evento.preventDefault();
      acciones.abrirRapida();
      return;
    }
    if (evento.key === 'Escape') {
      if (hayVelo()) return;
      const activo = document.activeElement;
      if (activo instanceof HTMLElement) activo.blur();
      if (get().detalleAbierto) set({ detalleAbierto: false });
      else if (get().texto) void quitarBusqueda();
      return;
    }
    if (control || evento.altKey || evento.shiftKey) return;
    if (hayVelo() || enEdicion()) return;
    if (evento.key === '/') {
      evento.preventDefault();
      acciones.abrirPaleta();
      return;
    }
    const tecla = evento.key.toLowerCase();
    const numero = Number.parseInt(tecla, 10);
    if (numero >= 1 && numero <= 5) {
      evento.preventDefault();
      const vista = VISTAS[numero - 1];
      if (vista) void irVista(vista);
      return;
    }
    if (tecla === 't') {
      evento.preventDefault();
      void irHoy();
      return;
    }
    if (evento.key === '[') {
      evento.preventDefault();
      const dia = diaAnteriorConEntradas();
      if (dia) void irDia(dia);
      return;
    }
    if (evento.key === ']') {
      evento.preventDefault();
      const dia = diaSiguienteConEntradas();
      if (dia) void irDia(dia);
      return;
    }
    if (evento.key === 'ArrowDown' || tecla === 'j') {
      evento.preventDefault();
      moverSeleccion(1);
      return;
    }
    if (evento.key === 'ArrowUp' || tecla === 'k') {
      evento.preventDefault();
      moverSeleccion(-1);
      return;
    }
    if (evento.key === 'Enter') {
      const entrada = entradaSeleccionada();
      if (!entrada) return;
      evento.preventDefault();
      if (entrada.url) void abrirEntrada(entrada.id);
      else void copiarEntrada(entrada);
      return;
    }
    if (evento.key === ' ') {
      evento.preventDefault();
      const estado = get();
      if (estado.seleccion === null && estado.entradas[0]) {
        set({ seleccion: estado.entradas[0].id, detalleAbierto: true });
        void cargarDetalle(estado.entradas[0].id);
        return;
      }
      set({ detalleAbierto: !estado.detalleAbierto });
      return;
    }
    if (tecla === 'n') {
      evento.preventDefault();
      const estado = get();
      const id = estado.seleccion ?? estado.entradas[0]?.id ?? null;
      if (id === null) return;
      if (estado.seleccion !== id) {
        set({ seleccion: id, detalleAbierto: true });
        void cargarDetalle(id).then(() => {
          document.querySelector<HTMLTextAreaElement>('.compositor textarea')?.focus();
        });
      } else {
        set({ detalleAbierto: true });
        window.setTimeout(
          () => document.querySelector<HTMLTextAreaElement>('.compositor textarea')?.focus(),
          20,
        );
      }
      return;
    }
    const entrada = entradaSeleccionada();
    if (!entrada) return;
    if (tecla === 'f') {
      evento.preventDefault();
      void fijar(entrada.id, !entrada.fijada);
      return;
    }
    if (tecla === 'e') {
      evento.preventDefault();
      void marcarVisto(entrada.id);
      return;
    }
    if (tecla === 'c') {
      evento.preventDefault();
      void copiarEntrada(entrada);
      return;
    }
    if (evento.key === 'Delete') {
      evento.preventDefault();
      if (get().vista === 'papelera') borrarDefinitivo(entrada.id);
      else void borrar(entrada.id);
      return;
    }
  });
}
