import { h } from '../dom';

interface OpcionesAviso {
  accion?: { texto: string; fn: () => void | Promise<void> };
  mal?: boolean;
  ms?: number;
}

let contenedor: HTMLElement | null = null;

function raiz(): HTMLElement {
  if (!contenedor) {
    contenedor = document.querySelector<HTMLElement>('.avisos') ?? h('div', { class: 'avisos' });
    if (!contenedor.isConnected) document.body.appendChild(contenedor);
  }
  return contenedor;
}

export function mostrarAviso(texto: string, opciones: OpcionesAviso = {}): void {
  const raizAvisos = raiz();
  const aviso = h('div', { class: `aviso${opciones.mal ? ' mal' : ''}${opciones.accion ? '' : ' solo'}` });
  aviso.appendChild(h('span', { text: texto }));
  if (opciones.accion) {
    const boton = h('button', {
      class: 'btn peq',
      text: opciones.accion.texto,
      onclick: () => {
        void opciones.accion?.fn();
        cerrar();
      },
    });
    aviso.appendChild(boton);
  }
  raizAvisos.appendChild(aviso);

  let cerrado = false;
  const cerrar = (): void => {
    if (cerrado) return;
    cerrado = true;
    aviso.classList.add('saliendo');
    window.setTimeout(() => aviso.remove(), 180);
  };
  window.setTimeout(cerrar, opciones.ms ?? 4000);
  aviso.addEventListener('click', (evento) => {
    if (evento.target === aviso) cerrar();
  });
}
