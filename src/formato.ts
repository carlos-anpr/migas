import type { Cambio, EntradaVista, Fuente, Trozo } from './types';

const FORMATO_SEMANA_LARGA = new Intl.DateTimeFormat('es-ES', { weekday: 'long' });
const FORMATO_SEMANA_CORTA = new Intl.DateTimeFormat('es-ES', { weekday: 'short' });
const FORMATO_MES_CORTO = new Intl.DateTimeFormat('es-ES', { month: 'short' });
const FORMATO_MES_LARGO = new Intl.DateTimeFormat('es-ES', { month: 'long' });
const FORMATO_HORA = new Intl.DateTimeFormat('es-ES', {
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
});

function capitalizar(texto: string): string {
  return texto.charAt(0).toUpperCase() + texto.slice(1);
}

function mesCorto(fecha: Date): string {
  return FORMATO_MES_CORTO.format(fecha).replace(/\.$/, '');
}

export function diaLocal(fecha = new Date()): string {
  return `${fecha.getFullYear()}-${String(fecha.getMonth() + 1).padStart(2, '0')}-${String(fecha.getDate()).padStart(2, '0')}`;
}

export function hoy(): string {
  return diaLocal();
}

export function parseDia(dia: string): Date {
  const [anio, mes, numero] = dia.split('-').map(Number);
  return new Date(anio, (mes ?? 1) - 1, numero ?? 1, 12);
}

export function sumarDias(dia: string, dias: number): string {
  const fecha = parseDia(dia);
  fecha.setDate(fecha.getDate() + dias);
  return diaLocal(fecha);
}

export function diaPartes(dia: string): { semana: string; resto: string } {
  const fecha = parseDia(dia);
  return {
    semana: `${capitalizar(FORMATO_SEMANA_LARGA.format(fecha))},`,
    resto: `${fecha.getDate()} ${mesCorto(fecha)}`,
  };
}

export function diaLargo(dia: string): string {
  const partes = diaPartes(dia);
  return `${partes.semana} ${partes.resto}`;
}

export function diaCorto(dia: string): string {
  const fecha = parseDia(dia);
  return `${FORMATO_SEMANA_CORTA.format(fecha)} ${fecha.getDate()} ${mesCorto(fecha)}`;
}

export function etiquetaDia(dia: string): string {
  const actual = hoy();
  if (dia === actual) return 'Hoy';
  if (dia === sumarDias(actual, -1)) {
    const fecha = parseDia(dia);
    return `Ayer · ${FORMATO_SEMANA_CORTA.format(fecha)} ${fecha.getDate()}`;
  }
  return diaCorto(dia);
}

export function mesLargo(mes: string): string {
  const [anio, numero] = mes.split('-').map(Number);
  const fecha = new Date(anio, (numero ?? 1) - 1, 1);
  return `${capitalizar(FORMATO_MES_LARGO.format(fecha))} ${anio}`;
}

export function hora(iso: string): string {
  const fecha = new Date(iso);
  if (Number.isNaN(fecha.getTime())) return '';
  return FORMATO_HORA.format(fecha);
}

export function hace(iso: string): string {
  const fecha = new Date(iso);
  if (Number.isNaN(fecha.getTime())) return iso;
  const minutos = Math.max(0, Math.floor((Date.now() - fecha.getTime()) / 60_000));
  if (minutos < 1) return 'ahora';
  if (minutos < 60) return `hace ${minutos} min`;
  const horas = Math.floor(minutos / 60);
  if (horas < 24) return `hace ${horas} h`;
  const dias = Math.floor(horas / 24);
  if (dias === 1) return 'ayer';
  return `hace ${dias} d`;
}

export function diaHora(iso: string): string {
  const fecha = new Date(iso);
  if (Number.isNaN(fecha.getTime())) return iso;
  const dia = diaLocal(fecha);
  const hhmm = hora(iso);
  if (dia === hoy()) return `hoy ${hhmm}`;
  if (dia === sumarDias(hoy(), -1)) return `ayer ${hhmm}`;
  return `${fecha.getDate()} ${mesCorto(fecha)} ${hhmm}`;
}

export function nivelDia(total: number): number {
  if (total >= 6) return 3;
  if (total >= 3) return 2;
  if (total >= 1) return 1;
  return 0;
}

export function iniciales(nombre: string): string {
  return nombre
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((parte) => parte[0]?.toUpperCase() ?? '')
    .join('');
}

export function resumenCambios(cambios: Cambio[]): string {
  return cambios
    .slice(0, 2)
    .map((cambio) => {
      switch (cambio.tipo) {
        case 'comentarios':
          if (cambio.autores.length === 1) return `${cambio.autores[0]} comentó`;
          if (cambio.n === 1) return '1 comentario';
          return `${cambio.n} comentarios`;
        case 'estado':
          return `Estado → ${cambio.despues ?? 'sin estado'}`;
        case 'asignado':
          return `Asignado → ${cambio.despues ?? 'nadie'}`;
        case 'titulo':
          return 'Título cambiado';
        case 'pipeline':
          return `Pipeline: ${cambio.antes ?? '—'} → ${cambio.despues ?? '—'}`;
        case 'fusionada':
          return 'Fusionada';
        case 'etiquetas': {
          const partes: string[] = [];
          if (cambio.anadidas.length) partes.push(`+${cambio.anadidas.join(', ')}`);
          if (cambio.quitadas.length) partes.push(`-${cambio.quitadas.join(', ')}`);
          return `Etiquetas: ${partes.join(' ')}`;
        }
      }
    })
    .join(' · ');
}

export function fuenteTexto(entrada: EntradaVista): string {
  switch (entrada.fuente) {
    case 'jira':
      return 'Jira';
    case 'gitlab':
      return 'GitLab';
    case 'fs':
      return 'Freshservice';
    case 'web':
      return entrada.host ?? 'Web';
    case 'texto':
      return 'Selección';
    case 'nota':
      return 'Nota';
  }
}

export function fuenteIcono(entrada: EntradaVista): string {
  switch (entrada.fuente) {
    case 'jira':
      return 'square-kanban';
    case 'fs':
      return 'life-buoy';
    case 'web':
      return 'globe';
    case 'texto':
      return 'text-quote';
    case 'nota':
      return 'pen-line';
    case 'gitlab':
      if (entrada.clave?.startsWith('WI')) return 'rocket';
      if (entrada.clave?.startsWith('!')) return 'git-merge';
      if (entrada.clave?.startsWith('#')) return 'circle-dot';
      return 'git-branch';
  }
}

export function textoSeleccion(entrada: EntradaVista): string | null {
  if (entrada.tipo === 'nota') return entrada.ultimaNota ?? entrada.seleccion;
  return entrada.seleccion;
}

export function urlCorta(url: string): string {
  return url.replace(/^https?:\/\//, '').replace(/\/$/, '');
}

export function esUrlUnica(texto: string): boolean {
  const limpio = texto.trim();
  return /^https?:\/\/\S+$/.test(limpio) && !/\s/.test(limpio);
}

export function hostDe(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
}

export function detectarClaves(
  texto: string,
  prefijos: string[] = ['PROJ'],
): Array<{ texto: string; fuerte: boolean }> {
  const salida: Array<{ texto: string; fuerte: boolean }> = [];
  const anadir = (valor: string, fuerte: boolean): void => {
    if (!salida.some((item) => item.texto === valor)) salida.push({ texto: valor, fuerte });
  };
  if (prefijos.length) {
    const patron = prefijos
      .map((prefijo) => prefijo.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
      .join('|');
    for (const match of texto.matchAll(new RegExp(`\\b(?:${patron})-\\d+\\b`, 'g'))) {
      anadir(match[0], true);
    }
  }
  for (const match of texto.matchAll(/\bFS\s*#?\s*\d{4,6}\b/gi)) anadir(match[0].replace(/\s+/g, ''), true);
  for (const match of texto.matchAll(/\bWI\s*\d+\b/gi)) anadir(match[0].replace(/\s+/g, ' '), true);
  for (const match of texto.matchAll(/!\d+\b/g)) anadir(match[0], true);
  for (const match of texto.matchAll(/https?:\/\/[^\s<>()]+/g)) anadir(match[0], false);
  return salida;
}

export function trozosDeFragmento(trozos: Trozo[]): DocumentFragment {
  const fragmento = document.createDocumentFragment();
  for (const trozo of trozos) {
    if (trozo.m) {
      const marca = document.createElement('mark');
      marca.textContent = trozo.t;
      fragmento.appendChild(marca);
    } else {
      fragmento.appendChild(document.createTextNode(trozo.t));
    }
  }
  return fragmento;
}

export function fuenteCorta(fuente: Fuente): string {
  switch (fuente) {
    case 'jira':
      return 'Jira';
    case 'gitlab':
      return 'GitLab';
    case 'fs':
      return 'FS';
    case 'web':
      return 'Web';
    case 'texto':
      return 'Texto';
    case 'nota':
      return 'Nota';
  }
}

export function pluralMigas(total: number): string {
  return `${total} ${total === 1 ? 'miga' : 'migas'}`;
}
