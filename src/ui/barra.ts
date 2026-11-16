import { arrastrar, alternarMaximizar, h, icono, logo, minimizar } from '../dom';
import { get, comprobarAhora, ocultarPrincipal } from '../estado';
import { hace } from '../formato';

export function pintarBarra(abrirPaleta: () => void, abrirAjustes: () => void): HTMLElement {
  const estado = get();
  const barra = h('header', { class: 'barra', 'data-tauri-drag-region': '', onmousedown: arrastrar });
  barra.appendChild(h('div', { class: 'marca' }, logo(), 'migas'));
  barra.appendChild(
    h(
      'button',
      { class: 'lanzador', 'data-abre': 'paleta', onclick: abrirPaleta },
      icono('search'),
      h('span', { text: 'Buscar en tu rastro…' }),
      h('kbd', { text: 'Ctrl' }),
      h('kbd', { text: 'K' }),
    ),
  );

  const derecha = h('div', { class: 'barra-der' });
  const seguimiento = estado.seguimiento ?? estado.estadoApp?.seguimiento ?? null;
  const enCurso = seguimiento?.enCurso ?? false;
  const errores = seguimiento?.errores ?? estado.estadoApp?.seguimiento.errores ?? [];
  let texto = 'Comprobado hace un momento';
  if (enCurso) texto = 'Comprobando…';
  else if (errores.length) texto = 'Sin conexión';
  else if (seguimiento?.ultima) texto = `Comprobado ${hace(seguimiento.ultima)}`;
  const vigia = h(
    'button',
    {
      class: `vigia${enCurso ? ' girando' : ''}${!enCurso && errores.length ? ' error' : ''}`,
      title: enCurso ? 'Comprobando cambios' : errores.length ? errores.join(' · ') : 'Comprobar ahora',
      onclick: () => void comprobarAhora(),
    },
    h('span', { class: 'luz' }),
    texto,
  );
  derecha.appendChild(vigia);
  derecha.appendChild(
    h(
      'button',
      { class: 'btn-ventana', title: 'Ajustes', 'aria-label': 'Ajustes', onclick: abrirAjustes },
      icono('settings-2'),
    ),
  );
  derecha.appendChild(
    h(
      'button',
      { class: 'btn-ventana', title: 'Minimizar', 'aria-label': 'Minimizar', onclick: () => void minimizar() },
      icono('minus'),
    ),
  );
  derecha.appendChild(
    h(
      'button',
      { class: 'btn-ventana', title: 'Maximizar', 'aria-label': 'Maximizar', onclick: () => void alternarMaximizar() },
      icono('square', 's'),
    ),
  );
  derecha.appendChild(
    h(
      'button',
      {
        class: 'btn-ventana cerrar',
        title: 'Cerrar (sigue en segundo plano)',
        'aria-label': 'Cerrar',
        onclick: () => void ocultarPrincipal(),
      },
      icono('x'),
    ),
  );
  barra.appendChild(derecha);
  return barra;
}
