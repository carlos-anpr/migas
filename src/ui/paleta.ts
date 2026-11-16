import type { EntradaVista, Filtro, Fuente } from '../types';
import { debounce, h, icono } from '../dom';
import {
  api,
  mostrarBusqueda,
  recargar,
  saltarA,
} from '../estado';
import {
  diaPartes,
  fuenteIcono,
  fuenteTexto,
  hora,
  hoy,
  sumarDias,
  trozosDeFragmento,
} from '../formato';
import { mostrarAviso } from './avisos';

type DiaFiltro = 'cualquier' | 'hoy' | 'semana';

let veloActual: HTMLElement | null = null;

export function cerrarPaleta(): void {
  veloActual?.remove();
  veloActual = null;
}

export function abrirPaleta(modoEnlazar?: number, consultaInicial = ''): void {
  cerrarPaleta();
  const velo = h('div', { class: 'velo paleta-velo' });
  const paleta = h('div', { class: 'paleta', role: 'dialog', 'aria-label': 'Buscar' });
  velo.appendChild(paleta);
  velo.addEventListener('click', (evento) => {
    if (evento.target === velo) cerrarPaleta();
  });
  veloActual = velo;
  document.body.appendChild(velo);

  let texto = consultaInicial;
  let fuentes: Fuente[] = [];
  let diaFiltro: DiaFiltro = 'cualquier';
  let resultados: EntradaVista[] = [];
  let indice = 0;
  let peticion = 0;

  const entrada = h('div', { class: 'paleta-entrada' });
  entrada.appendChild(icono('search', 'l'));
  const input = h('input', {
    placeholder: 'Busca por título, clave, app, texto o nota…',
    value: texto,
    'aria-label': 'Buscar',
  }) as HTMLInputElement;
  entrada.appendChild(input);
  if (modoEnlazar !== undefined) {
    entrada.appendChild(h('span', { class: 'paleta-modo', text: 'Enlazar con…' }));
  }
  entrada.appendChild(h('kbd', { text: 'Esc' }));
  paleta.appendChild(entrada);

  const filtros = h('div', { class: 'paleta-filtros' });
  paleta.appendChild(filtros);
  const lista = h('div', { class: 'paleta-res', role: 'listbox' });
  paleta.appendChild(lista);
  const pie = h('div', { class: 'paleta-pie' });
  pie.appendChild(h('span', {}, h('kbd', { text: '↑' }), h('kbd', { text: '↓' }), ' moverse'));
  pie.appendChild(h('span', {}, h('kbd', { text: '↵' }), ' ver'));
  pie.appendChild(h('span', {}, h('kbd', { text: 'Ctrl' }), h('kbd', { text: '↵' }), ' abrir en el navegador'));
  paleta.appendChild(pie);

  const pintarFiltros = (): void => {
    filtros.replaceChildren();
    for (const [valor, etiqueta] of [
      ['cualquier', 'Cualquier día'],
      ['hoy', 'Hoy'],
      ['semana', 'Esta semana'],
    ] as Array<[DiaFiltro, string]>) {
      filtros.appendChild(
        h('button', {
          class: 'chip',
          'aria-pressed': diaFiltro === valor ? 'true' : 'false',
          text: etiqueta,
          onclick: () => {
            diaFiltro = valor;
            pintarFiltros();
            void buscar();
          },
        }),
      );
    }
    filtros.appendChild(h('span', { class: 'sep' }));
    for (const fuente of ['jira', 'gitlab', 'fs'] as Fuente[]) {
      filtros.appendChild(
        h(
          'button',
          {
            class: 'chip',
            'data-fuente': fuente,
            'aria-pressed': fuentes.includes(fuente) ? 'true' : 'false',
            onclick: () => {
              fuentes = fuentes.includes(fuente)
                ? fuentes.filter((actual) => actual !== fuente)
                : [...fuentes, fuente];
              pintarFiltros();
              void buscar();
            },
          },
          h('span', { class: 'bola' }),
          fuente === 'fs' ? 'FS' : fuente === 'gitlab' ? 'GitLab' : 'Jira',
        ),
      );
    }
  };

  const inicioSemana = (): string => {
    const fecha = new Date();
    const desplazamiento = (fecha.getDay() + 6) % 7;
    return sumarDias(hoy(), -desplazamiento);
  };

  const buscar = async (): Promise<void> => {
    const numero = ++peticion;
    const filtro: Filtro = {
      vista: diaFiltro === 'hoy' ? 'dia' : 'todo',
      dia: diaFiltro === 'hoy' ? hoy() : null,
      texto: texto.trim() || null,
      fuentes,
      limite: texto.trim() ? 60 : 8,
    };
    try {
      const listado = await api.listar(filtro);
      if (numero !== peticion) return;
      resultados =
        diaFiltro === 'semana'
          ? listado.entradas.filter((item) => item.dia >= inicioSemana())
          : listado.entradas;
      indice = Math.min(indice, Math.max(0, resultados.length - 1));
      pintarResultados();
    } catch (error) {
      mostrarAviso(String(error), { mal: true });
    }
  };

  const seleccionarEntrada = async (entradaSeleccionada: EntradaVista): Promise<void> => {
    cerrarPaleta();
    const consulta = texto.trim();
    if (consulta) await mostrarBusqueda(consulta, entradaSeleccionada.id);
    else await saltarA(entradaSeleccionada.id, entradaSeleccionada.dia);
  };

  const elegir = (elegida: EntradaVista, abrir: boolean): void => {
    if (abrir) {
      cerrarPaleta();
      void api.abrir(elegida.id);
      return;
    }
    if (modoEnlazar !== undefined) {
      cerrarPaleta();
      void api.enlazar(modoEnlazar, elegida.id).then(() => {
        mostrarAviso('Enlazadas');
        return recargar();
      }).catch((error: unknown) => mostrarAviso(String(error), { mal: true }));
      return;
    }
    void seleccionarEntrada(elegida);
  };

  const pintarResultados = (): void => {
    lista.replaceChildren();
    if (resultados.length === 0) {
      lista.appendChild(h('div', { class: 'paleta-vacia', text: 'Nada coincide' }));
      return;
    }
    let diaActual = '';
    resultados.forEach((resultado, posicion) => {
      if (resultado.dia !== diaActual) {
        diaActual = resultado.dia;
        const partes = diaPartes(resultado.dia);
        lista.appendChild(
          h('div', { class: 'paleta-grupo', text: resultado.dia === hoy() ? 'Hoy' : `${partes.semana.replace(',', '')} ${partes.resto}` }),
        );
      }
      const res = h('div', {
        class: 'res',
        'data-fuente': resultado.fuente,
        role: 'option',
        'aria-selected': posicion === indice ? 'true' : 'false',
        // Un clic elige (como Enter); Ctrl+clic abre (como Ctrl+Enter).
        onclick: (evento: MouseEvent) => {
          indice = posicion;
          elegir(resultado, evento.ctrlKey || evento.metaKey);
        },
      });
      res.appendChild(h('span', { class: 'ico-fuente' }, icono(fuenteIcono(resultado))));
      const titulo = h('span', { class: 'res-titulo' });
      if (resultado.fragmento?.length) titulo.appendChild(trozosDeFragmento(resultado.fragmento));
      else titulo.textContent = resultado.titulo;
      res.appendChild(titulo);
      const derecha = h('span', { class: 'res-der' });
      if (resultado.cambios.length) derecha.appendChild(h('span', { class: 'cambio-luz' }));
      derecha.appendChild(h('span', { text: hora(resultado.creada) }));
      res.appendChild(derecha);
      const sub = h('span', { class: 'res-sub' });
      const partes: string[] = [];
      if (resultado.clave) partes.push(resultado.clave);
      if (resultado.estado) partes.push(resultado.estado);
      else if (resultado.app) partes.push(resultado.app);
      else if (resultado.host) partes.push(resultado.host);
      sub.textContent = partes.join(' · ') || fuenteTexto(resultado);
      res.appendChild(sub);
      lista.appendChild(res);
    });
  };

  const mover = (delta: number): void => {
    if (resultados.length === 0) return;
    indice = Math.min(Math.max(indice + delta, 0), resultados.length - 1);
    pintarResultados();
    const seleccionado = lista.querySelectorAll<HTMLElement>('.res')[indice];
    seleccionado?.scrollIntoView({ block: 'nearest' });
  };

  const buscarDebounce = debounce(() => void buscar(), 120);
  input.addEventListener('input', () => {
    texto = input.value;
    indice = 0;
    buscarDebounce();
  });

  input.addEventListener('keydown', (evento) => {
    if (evento.key === 'Escape') {
      evento.preventDefault();
      evento.stopPropagation();
      cerrarPaleta();
    } else if (evento.key === 'ArrowDown') {
      evento.preventDefault();
      mover(1);
    } else if (evento.key === 'ArrowUp') {
      evento.preventDefault();
      mover(-1);
    } else if (evento.key === 'Enter') {
      evento.preventDefault();
      const elegida = resultados[indice];
      if (!elegida) return;
      elegir(elegida, evento.ctrlKey || evento.metaKey);
    }
  });

  pintarFiltros();
  if (consultaInicial) input.value = consultaInicial;
  input.focus();
  input.select();
  void buscar();
}
