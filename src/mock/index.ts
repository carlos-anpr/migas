import type {
  Ajustes,
  Cambio,
  DetalleEntrada,
  EntradaVista,
  EstadoApp,
  Fuente,
  Nota,
  Relacion,
  Trozo,
} from '../types';
import type { Servicio } from '../estado';
import { ajustesDefecto } from '../estado';
import { hoy, sumarDias } from '../formato';

function fecha(hora: string, dia = hoy()): string {
  const [horas, minutos] = hora.split(':').map(Number);
  const fecha = new Date(`${dia}T00:00:00`);
  fecha.setHours(horas ?? 0, minutos ?? 0, 0, 0);
  return fecha.toISOString();
}

function crearEntrada(
  id: number,
  titulo: string,
  hora: string,
  datos: Partial<EntradaVista> = {},
): EntradaVista {
  return {
    id,
    dia: hoy(),
    hora,
    creada: fecha(hora),
    tipo: 'enlace',
    origen: 'navegador',
    fuente: 'web',
    url: null,
    titulo,
    clave: null,
    estado: null,
    app: null,
    proyecto: null,
    host: null,
    seleccion: null,
    notas: 0,
    ultimaNota: null,
    cambios: [],
    fijada: false,
    relacionadas: 0,
    capturas: 1,
    fragmento: null,
    borrada: null,
    ...datos,
  };
}

const cambiosJira: Cambio[] = [
  {
    tipo: 'comentarios',
    n: 1,
    autores: ['Ana García'],
    extracto: 'Desplegado en staging. Probado con la reserva de prueba.',
  },
  { tipo: 'estado', antes: 'En curso', despues: 'En pruebas' },
];

const cambiosMr: Cambio[] = [{ tipo: 'pipeline', antes: 'running', despues: 'success' }];

const entradas: EntradaVista[] = [
  crearEntrada(
    1,
    'Checkout: el total no cuadra cuando se aplican dos cupones',
    '16:42',
    {
      url: 'https://empresa.atlassian.net/browse/PROJ-1896',
      titulo: 'Checkout: el total no cuadra cuando se aplican dos cupones',
      clave: 'PROJ-1896',
      estado: 'En pruebas',
      app: 'reservas-api',
      proyecto: 'PROJ',
      fuente: 'jira',
      notas: 2,
      fijada: true,
      relacionadas: 4,
      cambios: cambiosJira,
    },
  ),
  crearEntrada(
    2,
    'fix(checkout): aplicar los cupones en orden antes de redondear',
    '15:10',
    {
      url: 'https://gitlab.com/equipo/reservas-api/-/merge_requests/39',
      clave: '!39',
      estado: 'opened',
      app: 'reservas-api',
      proyecto: 'equipo/reservas-api',
      fuente: 'gitlab',
      cambios: cambiosMr,
      relacionadas: 3,
    },
  ),
  crearEntrada(
    3,
    'ORA-01795: maximum number of expressions in a list is 1000',
    '13:27',
    {
      tipo: 'texto',
      origen: 'global',
      fuente: 'texto',
      seleccion:
        'ORA-01795: maximum number of expressions in a list is 1000 — partir el IN en bloques de 900 en obtenerReservas()',
      relacionadas: 1,
    },
  ),
  crearEntrada(
    4,
    '[staging] Despliegue reservas-api 1.4.2',
    '12:05',
    {
      url: 'https://gitlab.com/equipo/despliegues/-/work_items/1234',
      clave: 'WI 1234',
      estado: 'opened',
      app: 'reservas-api',
      proyecto: 'equipo/despliegues',
      fuente: 'gitlab',
      notas: 1,
      ultimaNota: 'Pendiente de confirmar las variables nuevas',
      relacionadas: 4,
    },
  ),
  crearEntrada(5, 'No llega el correo de confirmación de algunas reservas', '10:48', {
    url: 'https://empresa.freshservice.com/a/tickets/45600',
    clave: 'FS#45600',
    estado: 'Pendiente',
    fuente: 'fs',
    capturas: 3,
    relacionadas: 3,
  }),
  crearEntrada(6, 'rusqlite::Connection — Rust', '09:31', {
    url: 'https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html',
    fuente: 'web',
    host: 'docs.rs',
    seleccion:
      'busy_timeout: Set a busy handler that sleeps for a specified amount of time when a table is locked.',
  }),
  crearEntrada(7, 'Daily: preguntar a Ana por el criterio de redondeo', '08:55', {
    tipo: 'nota',
    origen: 'global',
    fuente: 'nota',
    ultimaNota:
      'Daily: preguntar a Ana por el criterio de redondeo y revisar la cola de soporte antes de las 12.',
  }),
  crearEntrada(8, 'Revisar la validación del código postal en el alta', '08:10', {
    url: 'https://empresa.atlassian.net/browse/PROJ-1850',
    clave: 'PROJ-1850',
    estado: 'En curso',
    app: 'reservas-api',
    proyecto: 'PROJ',
    fuente: 'jira',
  }),
  crearEntrada(9, 'Informes: excluir las reservas canceladas', '07:45', {
    url: 'https://gitlab.com/equipo/informes-web/-/issues/12',
    clave: '#12',
    estado: 'opened',
    app: 'informes-web',
    proyecto: 'equipo/informes-web',
    fuente: 'gitlab',
  }),
];

const entradaAyer = crearEntrada(
  10,
  'Checkout: el total no cuadra cuando se aplican dos cupones',
  '18:20',
  {
    dia: sumarDias(hoy(), -1),
    creada: fecha('18:20', sumarDias(hoy(), -1)),
    url: 'https://empresa.atlassian.net/browse/PROJ-1896',
    clave: 'PROJ-1896',
    estado: 'En curso',
    app: 'reservas-api',
    proyecto: 'PROJ',
    fuente: 'jira',
    titulo: 'Checkout: el total no cuadra cuando se aplican dos cupones',
  },
);

const todas = [...entradas, entradaAyer];

const notasJira: Nota[] = [
  {
    id: 1,
    entradaId: 1,
    creada: fecha('16:44'),
    editada: null,
    tipo: 'cita',
    texto: 'el descuento se aplica dos veces: 52,30 € → 47,07 € → 42,36 €',
  },
  {
    id: 2,
    entradaId: 1,
    creada: fecha('16:45'),
    editada: fecha('16:52'),
    tipo: 'nota',
    texto: 'Probar con el carrito de dos cupones antes de pasar a producción.',
  },
];

const comentariosJira = [
  {
    id: 'c1',
    autor: 'Ana García',
    mio: false,
    fecha: fecha('09:12'),
    extracto:
      'Desplegado en staging. Con la reserva de prueba ya cuadra el total, pero con cupones dobles sigue fallando.',
  },
  {
    id: 'c2',
    autor: 'Tú',
    mio: true,
    fecha: fecha('08:58'),
    extracto: 'MR !39 lista para revisar.',
  },
];

function recursoDe(entrada: EntradaVista): DetalleEntrada['recurso'] {
  if (!entrada.clave || entrada.fuente === 'texto' || entrada.fuente === 'nota' || entrada.fuente === 'web') {
    return null;
  }
  const tipo =
    entrada.fuente === 'jira' ? 'jira' : entrada.fuente === 'fs' ? 'fs' : entrada.clave.startsWith('!') ? 'mr' : 'issue';
  return {
    clave:
      tipo === 'jira'
        ? `jira:${entrada.clave}`
        : tipo === 'fs'
          ? `fs:${entrada.clave.replace('FS#', '')}`
          : tipo === 'mr'
            ? `mr:equipo/reservas-api!${entrada.clave.replace('!', '')}`
            : `issue:equipo/despliegues#${entrada.clave.replace('#', '').replace('WI ', '')}`,
    tipo,
    url: entrada.url ?? '',
    titulo: entrada.titulo,
    estado: entrada.estado,
    asignado: entrada.clave === 'PROJ-1896' ? 'Tú' : null,
    comprobado: new Date(Date.now() - 2 * 60 * 1000).toISOString(),
    error: null,
    visto: entrada.clave === 'PROJ-1896' ? fecha('08:58') : null,
    seguir: true,
    cambios: entrada.cambios,
    ultimos: entrada.clave === 'PROJ-1896' ? comentariosJira : [],
    nuevos: entrada.clave === 'PROJ-1896' ? ['c1'] : [],
  };
}

const relacionadasJira: Relacion[] = [todas[1], todas[3], todas[4], entradaAyer].map(
  (entrada, indice) => ({
    entrada,
    motivos: indice === 0 ? ['PROJ-1896', 'App reservas-api'] : indice === 1 ? ['App reservas-api'] : indice === 2 ? ['FS#45600'] : ['La misma, el 3 nov'],
    peso: indice === 3 ? 90 : 40,
    manual: false,
  }),
);

const detalles = new Map<number, DetalleEntrada>();
for (const entrada of todas) {
  detalles.set(entrada.id, {
    entrada,
    seleccion: entrada.id === 3 ? entrada.seleccion : null,
    notas: entrada.id === 1 ? notasJira : [],
    relacionadas: entrada.id === 1 ? relacionadasJira : [],
    historial:
      entrada.id === 1
        ? [
            { id: 10, dia: sumarDias(hoy(), -1), hora: '18:20' },
            { id: 11, dia: sumarDias(hoy(), -2), hora: '12:40' },
          ]
        : [],
    recurso: recursoDe(entrada),
  });
}

let ajustes: Ajustes = {
  ...ajustesDefecto(),
  apps: ['informes-web', 'reservas-api'],
};

function filtrar(filtro: Parameters<Servicio['listar']>[0]): EntradaVista[] {
  const dia = filtro.dia ?? hoy();
  let resultado = todas.filter((entrada) => {
    if (filtro.vista === 'papelera') return entrada.borrada !== null;
    if (entrada.borrada !== null) return false;
    switch (filtro.vista) {
      case 'dia':
        return entrada.dia === dia;
      case 'cambios':
        return entrada.cambios.length > 0;
      case 'fijadas':
        return entrada.fijada;
      case 'todo':
      default:
        return true;
    }
  });
  if (filtro.fuentes?.length) {
    resultado = resultado.filter((entrada) => filtro.fuentes?.includes(entrada.fuente));
  }
  const texto = filtro.texto?.trim().toLowerCase();
  if (texto) {
    resultado = resultado.filter((entrada) =>
      [entrada.titulo, entrada.clave, entrada.app, entrada.seleccion, entrada.ultimaNota, entrada.estado]
        .filter(Boolean)
        .some((valor) => valor?.toLowerCase().includes(texto)),
    );
  }
  return resultado.sort((a, b) => b.creada.localeCompare(a.creada));
}

function fragmento(entrada: EntradaVista, texto: string): Trozo[] | null {
  if (!texto) return null;
  const posicion = entrada.titulo.toLowerCase().indexOf(texto.toLowerCase());
  if (posicion < 0) return null;
  return [
    { t: entrada.titulo.slice(0, posicion), m: false },
    { t: entrada.titulo.slice(posicion, posicion + texto.length), m: true },
    { t: entrada.titulo.slice(posicion + texto.length), m: false },
  ].filter((trozo) => trozo.t.length > 0);
}

function estadoApp(): EstadoApp {
  return {
    version: '0.1.0',
    seguimiento: {
      enCurso: false,
      ultima: new Date(Date.now() - 2 * 60 * 1000).toISOString(),
      proxima: new Date(Date.now() + 3 * 60 * 1000).toISOString(),
      seguidos: 4,
      errores: [],
    },
    fuentes: [
      { fuente: 'jira', configurada: true, error: null },
      { fuente: 'gitlab', configurada: true, error: null },
      { fuente: 'fs', configurada: false, error: null },
    ],
    integraciones: {
      hostRegistrado: true,
      extensionVista: new Date().toISOString(),
      dobleCtrl: 'activo',
      autostart: true,
    },
    cambios: todas.filter((entrada) => entrada.borrada === null && entrada.cambios.length > 0).length,
    rutaBd: '~/.local/share/migas/migas.db',
    iaDisponible: true,
  };
}

function entradaPorId(id: number): EntradaVista | undefined {
  return todas.find((entrada) => entrada.id === id);
}

function actualizarEntrada(id: number, cambios: Partial<EntradaVista>): void {
  const indice = todas.findIndex((entrada) => entrada.id === id);
  if (indice < 0) return;
  todas[indice] = { ...todas[indice], ...cambios };
  const detalle = detalles.get(id);
  if (detalle) detalle.entrada = todas[indice];
}

export function crearMock(): Servicio {
  return {
    async listarDias(desde: string | null, hasta: string | null) {
      const mapa = new Map<string, { total: number; cambios: number }>();
      for (const entrada of todas) {
        if (entrada.borrada !== null) continue;
        if (desde && entrada.dia < desde) continue;
        if (hasta && entrada.dia > hasta) continue;
        const actual = mapa.get(entrada.dia) ?? { total: 0, cambios: 0 };
        actual.total += 1;
        if (entrada.cambios.length) actual.cambios += 1;
        mapa.set(entrada.dia, actual);
      }
      return [...mapa.entries()]
        .sort(([a], [b]) => b.localeCompare(a))
        .map(([dia, datos]) => ({ dia, total: datos.total, cambios: datos.cambios }));
    },
    async listar(filtro) {
      const resultado = filtrar(filtro).map((entrada) => ({
        ...entrada,
        fragmento: fragmento(entrada, filtro.texto ?? ''),
      }));
      const total = resultado.length;
      return { entradas: resultado.slice(0, filtro.limite ?? 300), total };
    },
    async detalle(id) {
      const detalle = detalles.get(id);
      if (!detalle) throw new Error('No existe la entrada');
      return detalle;
    },
    async abrir() {
      // En el mock no hay navegador que abrir.
    },
    async marcarVisto(id) {
      actualizarEntrada(id, { cambios: [] });
      const detalle = detalles.get(id);
      if (detalle?.recurso) detalle.recurso = { ...detalle.recurso, cambios: [], nuevos: [] };
    },
    async marcarTodoVisto() {
      let marcadas = 0;
      for (const entrada of todas) {
        if (entrada.cambios.length) {
          actualizarEntrada(entrada.id, { cambios: [] });
          const detalle = detalles.get(entrada.id);
          if (detalle?.recurso) detalle.recurso = { ...detalle.recurso, cambios: [], nuevos: [] };
          marcadas += 1;
        }
      }
      return marcadas;
    },
    async crearEntrada(nueva) {
      const id = Math.max(...todas.map((entrada) => entrada.id)) + 1;
      const url = nueva.url ?? null;
      const fuente: Fuente = url?.includes('atlassian.net')
        ? 'jira'
        : url?.includes('gitlab')
          ? 'gitlab'
          : url?.includes('freshservice.com')
            ? 'fs'
            : url
              ? 'web'
              : nueva.tipo === 'nota'
                ? 'nota'
                : 'texto';
      const titulo =
        nueva.titulo?.trim() ||
        nueva.seleccion?.trim().slice(0, 80) ||
        nueva.nota?.trim().slice(0, 80) ||
        'Sin título';
      const entrada = crearEntrada(id, titulo, new Date().toTimeString().slice(0, 5), {
        tipo: nueva.tipo,
        origen: nueva.origen,
        fuente,
        url,
        host: url ? new URL(url).host : null,
        seleccion: nueva.seleccion ?? null,
        notas: nueva.nota ? 1 : 0,
        ultimaNota: nueva.nota ?? null,
      });
      if (nueva.nota) {
        entrada.notas = 1;
        entrada.ultimaNota = nueva.nota;
      }
      todas.unshift(entrada);
      detalles.set(id, {
        entrada,
        seleccion: nueva.tipo === 'texto' ? nueva.seleccion : null,
        notas: nueva.nota
          ? [{ id: Date.now(), entradaId: id, creada: entrada.creada, editada: null, tipo: 'nota', texto: nueva.nota }]
          : [],
        relacionadas: [],
        historial: [],
        recurso: recursoDe(entrada),
      });
      return {
        id,
        duplicada: false,
        titulo,
        fuente,
        clave: null,
        cambios: 0,
        relacionadas: 0,
      };
    },
    async editarTitulo(id, titulo) {
      actualizarEntrada(id, { titulo });
    },
    async fijar(id, fijada) {
      actualizarEntrada(id, { fijada });
    },
    async borrar(id) {
      actualizarEntrada(id, { borrada: new Date().toISOString() });
    },
    async restaurar(id) {
      actualizarEntrada(id, { borrada: null });
    },
    async borrarDefinitivo(id, sello) {
      const indice = todas.findIndex((entrada) => entrada.id === id);
      const entrada = todas[indice];
      if (!entrada || entrada.borrada === null) {
        throw new Error('Solo se puede borrar definitivamente desde la papelera');
      }
      if (entrada.borrada !== sello) throw new Error('Esa miga ha cambiado; vuelve a intentarlo');
      todas.splice(indice, 1);
      detalles.delete(id);
    },
    async vaciarPapelera() {
      let borradas = 0;
      for (let indice = todas.length - 1; indice >= 0; indice -= 1) {
        const entrada = todas[indice];
        if (!entrada || entrada.borrada === null) continue;
        todas.splice(indice, 1);
        detalles.delete(entrada.id);
        borradas += 1;
      }
      return borradas;
    },
    async crearNota(entradaId, texto) {
      const detalle = detalles.get(entradaId);
      if (!detalle) throw new Error('No existe la entrada');
      const nota: Nota = {
        id: Date.now(),
        entradaId,
        creada: new Date().toISOString(),
        editada: null,
        tipo: 'nota',
        texto,
      };
      detalle.notas.push(nota);
      actualizarEntrada(entradaId, { notas: detalle.notas.length, ultimaNota: texto });
      return nota;
    },
    async editarNota(id, texto) {
      for (const detalle of detalles.values()) {
        const nota = detalle.notas.find((actual) => actual.id === id);
        if (nota) {
          nota.texto = texto;
          nota.editada = new Date().toISOString();
          return nota;
        }
      }
      throw new Error('No existe la nota');
    },
    async borrarNota(id) {
      for (const detalle of detalles.values()) {
        const indice = detalle.notas.findIndex((actual) => actual.id === id);
        if (indice >= 0) {
          detalle.notas.splice(indice, 1);
          return;
        }
      }
      throw new Error('No existe la nota');
    },
    async enlazar() {
      // Sin persistencia en el mock.
    },
    async desenlazar() {},
    async descartar() {},
    async seguir(id, seguir) {
      const detalle = detalles.get(id);
      if (detalle?.recurso) detalle.recurso = { ...detalle.recurso, seguir };
    },
    async comprobarAhora() {},
    async estadoApp() {
      return estadoApp();
    },
    async obtenerAjustes() {
      return ajustes;
    },
    async guardarAjustes(nuevos) {
      ajustes = { ...ajustes, ...nuevos };
    },
    async instalarIntegraciones() {
      return [
        '✓ Host nativo registrado (~/.config/google-chrome/NativeMessagingHosts/es.migas.host.json)',
        '✓ Icono copiado (~/.local/share/migas/migas.png)',
        '✓ Lanzador creado (~/.local/share/applications/migas.desktop)',
        '✓ Autostart creado (~/.config/autostart/migas.desktop)',
      ].join('\n');
    },
    async rutaExtension() {
      return '~/Proyectos/migas/extension';
    },
    async copiar(texto) {
      await navigator.clipboard?.writeText(texto);
    },
    async capturaPendiente() {
      return {
        primaria:
          'ORA-01795: maximum number of expressions in a list is 1000 — partir el IN en bloques de 900 en obtenerReservas() de informes-web (PROJ-1896)',
        portapapeles: null,
      };
    },
    async cerrarRapida() {},
    async abrirRapida() {},
    async ocultarPrincipal() {},
    async mostrarPrincipal() {},
    async mostrarPendiente() {
      return null;
    },
    async resumenDia() {
      return null;
    },
    async generarResumen() {
      throw new Error('La IA llega en F6');
    },
    async salir() {},
  };
}

export { entradaPorId };
