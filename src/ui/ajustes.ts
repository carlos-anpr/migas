import type { Ajustes, Fuente } from '../types';
import { debounce, h, icono } from '../dom';
import {
  api,
  aplicarTema,
  get,
  guardarAjustes,
} from '../estado';

type Seccion = 'conexion' | 'seguimiento' | 'captura' | 'apariencia' | 'ia';

const SECCIONES: Array<{ id: Seccion; texto: string; icono: string }> = [
  { id: 'conexion', texto: 'Conexión', icono: 'plug' },
  { id: 'seguimiento', texto: 'Seguimiento', icono: 'activity' },
  { id: 'captura', texto: 'Captura', icono: 'keyboard' },
  { id: 'apariencia', texto: 'Apariencia', icono: 'palette' },
  { id: 'ia', texto: 'IA', icono: 'sparkles' },
];

let veloActual: HTMLElement | null = null;
let alTeclearActual: ((evento: KeyboardEvent) => void) | null = null;

export function cerrarAjustes(): void {
  veloActual?.remove();
  veloActual = null;
  if (alTeclearActual) {
    document.removeEventListener('keydown', alTeclearActual);
    alTeclearActual = null;
  }
}

export function abrirAjustes(seccionInicial: string = 'conexion'): void {
  cerrarAjustes();
  let seccion: Seccion = SECCIONES.some((item) => item.id === seccionInicial)
    ? (seccionInicial as Seccion)
    : 'conexion';
  const pendiente: Partial<Ajustes> = {};
  const guardar = debounce(() => {
    if (Object.keys(pendiente).length === 0) return;
    const cambios = { ...pendiente };
    for (const clave of Object.keys(pendiente) as Array<keyof Ajustes>) delete pendiente[clave];
    void guardarAjustes(cambios);
  }, 400);
  const cambiar = <K extends keyof Ajustes>(clave: K, valor: Ajustes[K]): void => {
    pendiente[clave] = valor;
    guardar();
  };

  const velo = h('div', { class: 'velo velo-ajustes' });
  const dialogo = h('div', { class: 'dialogo', role: 'dialog', 'aria-label': 'Ajustes' });
  velo.appendChild(dialogo);
  veloActual = velo;
  document.body.appendChild(velo);
  velo.addEventListener('click', (evento) => {
    if (evento.target === velo) cerrarAjustes();
  });

  const cabecera = h('div', { class: 'dialogo-cab' }, h('h2', { text: 'Ajustes' }));
  cabecera.appendChild(
    h(
      'button',
      { class: 'btn-ventana', 'aria-label': 'Cerrar ajustes', onclick: cerrarAjustes },
      icono('x'),
    ),
  );
  dialogo.appendChild(cabecera);

  const nav = h('div', { class: 'dialogo-nav' });
  const cuerpo = h('div', { class: 'dialogo-cuerpo' });
  const pintarNav = (): void => {
    nav.replaceChildren();
    for (const item of SECCIONES) {
      nav.appendChild(
        h(
          'a',
          {
            class: 'nav-item',
            href: '#',
            'aria-current': item.id === seccion ? 'true' : undefined,
            onclick: (evento: Event) => {
              evento.preventDefault();
              seccion = item.id;
              pintarNav();
              document.getElementById(`ajuste-${item.id}`)?.scrollIntoView({ block: 'start' });
            },
          },
          icono(item.icono),
          item.texto,
        ),
      );
    }
  };

  const pintarCuerpo = (): void => {
    cuerpo.replaceChildren();
    cuerpo.appendChild(pintarConexion());
    cuerpo.appendChild(pintarFuentes());
    cuerpo.appendChild(pintarSeguimiento(cambiar));
    cuerpo.appendChild(pintarApariencia(cambiar));
    cuerpo.appendChild(pintarCaptura(cambiar));
    cuerpo.appendChild(pintarIa(cambiar));
    cuerpo.appendChild(pintarDatos());
  };

  dialogo.appendChild(nav);
  dialogo.appendChild(cuerpo);
  pintarNav();
  pintarCuerpo();
  if (seccionInicial === 'fuentes') {
    window.setTimeout(() => {
      document.getElementById('ajuste-fuentes')?.scrollIntoView({ block: 'start' });
    }, 0);
  }

  const alTeclear = (evento: KeyboardEvent): void => {
    if (evento.key === 'Escape') {
      evento.preventDefault();
      evento.stopPropagation();
      cerrarAjustes();
    }
  };
  alTeclearActual = alTeclear;
  document.addEventListener('keydown', alTeclear);
}

function pintarConexion(): HTMLElement {
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-conexion' });
  grupo.appendChild(h('h3', { text: 'Conectar el navegador' }));
  const pasos = h('ol', { class: 'pasos' });
  const paso1 = h('li');
  const informe = h('div');
  const instalar = h('button', {
    class: 'btn peq primario',
    text: 'Instalar integraciones',
    onclick: async () => {
      try {
        const lineas = await api.instalarIntegraciones();
        informe.replaceChildren(...lineas.split('\n').map((linea) => h('div', { class: 'ruta', text: linea })));
      } catch (error) {
        informe.replaceChildren(h('div', { class: 'ruta', text: String(error) }));
      }
    },
  });
  paso1.appendChild(
    h('div', {}, 'Registra migas en el sistema. ', instalar, informe),
  );
  pasos.appendChild(paso1);

  const ruta = h('span', { text: 'Cargando…' });
  const copiarRuta = h('button', {
    class: 'btn peq',
    text: 'Copiar',
    onclick: () => void api.copiar(ruta.textContent ?? ''),
  });
  void api
    .rutaExtension()
    .then((valor) => {
      ruta.textContent = valor;
    })
    .catch(() => {
      ruta.textContent = '(no disponible)';
    });
  const paso2 = h('li');
  paso2.appendChild(
    h(
      'div',
      {},
      'Abre ',
      h('span', { class: 'mono', text: 'chrome://extensions' }),
      ', activa ',
      h('b', { text: 'Modo desarrollador' }),
      ' y pulsa ',
      h('b', { text: 'Cargar descomprimida' }),
      ' con esta carpeta:',
      h('div', { class: 'ruta' }, ruta, copiarRuta),
    ),
  );
  pasos.appendChild(paso2);

  const paso3 = h('li');
  paso3.appendChild(
    h(
      'div',
      {},
      'Pulsa ',
      h('span', { class: 'gesto' }, h('kbd', { text: 'Ctrl' }), h('kbd', { text: 'Ctrl' })),
      ' en cualquier página. Verás el aviso de guardado.',
    ),
  );
  pasos.appendChild(paso3);
  grupo.appendChild(pasos);
  return grupo;
}

function pintarFuentes(): HTMLElement {
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-fuentes' });
  grupo.appendChild(h('h3', { text: 'Fuentes' }));
  grupo.appendChild(
    h('small', { text: 'Credenciales: variables de entorno o ~/.config/migas/credenciales.env' }),
  );
  const estado = get().estadoApp;
  const fuentes: Array<[Fuente, string]> = [
    ['jira', 'Jira'],
    ['gitlab', 'GitLab'],
    ['fs', 'Freshservice'],
  ];
  for (const [fuente, nombre] of fuentes) {
    const configurada = estado?.fuentes.find((item) => item.fuente === fuente)?.configurada ?? false;
    const fila = h('div', { class: 'ajuste' }, h('span', { class: 'et', text: nombre }));
    fila.appendChild(
      h(
        'span',
        { class: `integ ${configurada ? 'ok' : 'alerta'}` },
        h('span', { class: 'luz' }),
        configurada ? 'Credenciales encontradas' : 'Sin clave',
      ),
    );
    grupo.appendChild(fila);
  }
  return grupo;
}

function pintarSeguimiento(cambiar: <K extends keyof Ajustes>(clave: K, valor: Ajustes[K]) => void): HTMLElement {
  const ajustes = get().ajustes;
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-seguimiento' });
  grupo.appendChild(h('h3', { text: 'Seguimiento' }));
  const intervalo = h('input', {
    class: 'campo corto',
    type: 'number',
    min: '1',
    value: String(ajustes.intervaloMin),
    onchange: (evento: Event) => cambiar('intervaloMin', Number((evento.target as HTMLInputElement).value) || 1),
  });
  grupo.appendChild(
    h('div', { class: 'ajuste' }, h('label', { text: 'Comprobar cambios cada' }), h('span', {}, intervalo, ' min')),
  );
  const dias = h('input', {
    class: 'campo corto',
    type: 'number',
    min: '0',
    value: String(ajustes.diasSeguimiento),
    onchange: (evento: Event) => cambiar('diasSeguimiento', Number((evento.target as HTMLInputElement).value) || 0),
  });
  const filaDias = h('div', { class: 'ajuste' }, h('label', { text: 'Seguir lo guardado en los últimos' }), h('span', {}, dias, ' días'));
  grupo.appendChild(filaDias);
  const notificar = h('input', {
    type: 'checkbox',
    checked: ajustes.notificar,
    onchange: (evento: Event) => cambiar('notificar', (evento.target as HTMLInputElement).checked),
  });
  grupo.appendChild(h('div', { class: 'ajuste' }, h('label', { text: 'Avisar con notificación' }), h('label', { class: 'interruptor' }, notificar)));
  return grupo;
}

function pintarCaptura(cambiar: <K extends keyof Ajustes>(clave: K, valor: Ajustes[K]) => void): HTMLElement {
  const ajustes = get().ajustes;
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-captura' });
  grupo.appendChild(h('h3', { text: 'Captura' }));
  const doble = h('input', {
    type: 'checkbox',
    checked: ajustes.dobleCtrl,
    onchange: (evento: Event) => cambiar('dobleCtrl', (evento.target as HTMLInputElement).checked),
  });
  grupo.appendChild(h('div', { class: 'ajuste' }, h('label', { text: 'Doble Ctrl global' }), h('label', { class: 'interruptor' }, doble)));
  const ms = h('input', {
    class: 'campo corto',
    type: 'number',
    min: '100',
    max: '800',
    value: String(ajustes.dobleCtrlMs),
    onchange: (evento: Event) => cambiar('dobleCtrlMs', Number((evento.target as HTMLInputElement).value) || 330),
  });
  grupo.appendChild(h('div', { class: 'ajuste' }, h('label', { text: 'Segunda pulsación en' }), h('span', {}, ms, ' ms')));
  const explicacion = h('div', { class: 'ajuste' });
  explicacion.appendChild(
    h(
      'small',
      {},
      'Si el teclado no da permiso, crea un atajo propio en GNOME → Configuración → Teclado con el comando ',
      h('span', { class: 'mono', text: 'migas --capturar' }),
      '.',
    ),
  );
  grupo.appendChild(explicacion);
  return grupo;
}

function pintarApariencia(cambiar: <K extends keyof Ajustes>(clave: K, valor: Ajustes[K]) => void): HTMLElement {
  const ajustes = get().ajustes;
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-apariencia' });
  grupo.appendChild(h('h3', { text: 'Apariencia' }));
  const segmentado = h('div', { class: 'segmentado' });
  for (const [valor, texto] of [
    ['sistema', 'Sistema'],
    ['oscuro', 'Oscuro'],
    ['claro', 'Claro'],
  ] as Array<[Ajustes['tema'], string]>) {
    segmentado.appendChild(
      h('button', {
        'aria-pressed': ajustes.tema === valor ? 'true' : 'false',
        text: texto,
        onclick: () => {
          aplicarTema(valor);
          cambiar('tema', valor);
          segmentado.querySelectorAll<HTMLButtonElement>('button').forEach((boton) => {
            boton.setAttribute('aria-pressed', boton.textContent === texto ? 'true' : 'false');
          });
        },
      }),
    );
  }
  grupo.appendChild(h('div', { class: 'ajuste' }, h('span', { class: 'et', text: 'Tema' }), segmentado));
  return grupo;
}

function pintarIa(cambiar: <K extends keyof Ajustes>(clave: K, valor: Ajustes[K]) => void): HTMLElement {
  const ajustes = get().ajustes;
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-ia' });
  grupo.appendChild(h('h3', { text: 'IA' }));
  const ia = h('input', {
    type: 'checkbox',
    checked: ajustes.ia,
    onchange: (evento: Event) => cambiar('ia', (evento.target as HTMLInputElement).checked),
  });
  grupo.appendChild(h('div', { class: 'ajuste' }, h('label', { text: 'Usar IA para resumir el día' }), h('label', { class: 'interruptor' }, ia)));
  const modelo = h('input', {
    class: 'campo',
    value: ajustes.iaModelo,
    onchange: (evento: Event) => cambiar('iaModelo', (evento.target as HTMLInputElement).value),
  });
  grupo.appendChild(h('div', { class: 'ajuste' }, h('label', { text: 'Modelo' }), modelo));
  const base = h('input', {
    class: 'campo',
    value: ajustes.iaBaseUrl,
    onchange: (evento: Event) => cambiar('iaBaseUrl', (evento.target as HTMLInputElement).value),
  });
  grupo.appendChild(h('div', { class: 'ajuste' }, h('label', { text: 'URL base' }), base));
  return grupo;
}

function pintarDatos(): HTMLElement {
  const grupo = h('div', { class: 'ajuste-grupo', id: 'ajuste-datos' });
  grupo.appendChild(h('h3', { text: 'Datos' }));
  const ruta = h('div', { class: 'ruta' }, h('span', { text: get().estadoApp?.rutaBd ?? '' }));
  grupo.appendChild(h('div', { class: 'ajuste' }, h('span', { class: 'et', text: 'Base de datos' }), ruta));
  grupo.appendChild(
    h(
      'button',
      {
        class: 'btn',
        text: 'Salir de migas',
        onclick: () => void api.salir(),
      },
    ),
  );
  return grupo;
}
