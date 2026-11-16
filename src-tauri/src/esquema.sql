-- migas · esquema v1 (canónico, del líder). Se aplica si PRAGMA user_version < 1.

CREATE TABLE IF NOT EXISTS recurso (
  clave          TEXT PRIMARY KEY,          -- jira:PROJ-1 | mr:ruta!39 | issue:ruta#12 | fs:45600
  tipo           TEXT NOT NULL,             -- jira | mr | issue | fs
  url            TEXT NOT NULL,             -- canónica
  titulo         TEXT,
  estado         TEXT,
  proyecto       TEXT,                      -- ruta GitLab o prefijo Jira
  app            TEXT,
  seguir         INTEGER NOT NULL DEFAULT 1,
  huella_actual  TEXT,                      -- JSON model::Huella
  huella_vista   TEXT,                      -- JSON model::Huella
  pendiente_visto INTEGER NOT NULL DEFAULT 0,
  cambios        TEXT,                      -- JSON Vec<model::Cambio>, NULL = sin cambios
  comprobado     TEXT,                      -- RFC 3339 de la última consulta correcta
  error          TEXT,
  visto          TEXT
);

CREATE TABLE IF NOT EXISTS entrada (
  id          INTEGER PRIMARY KEY,
  dia         TEXT NOT NULL,                -- YYYY-MM-DD local
  creada      TEXT NOT NULL,
  actualizada TEXT NOT NULL,
  tipo        TEXT NOT NULL CHECK (tipo IN ('enlace','texto','nota')),
  origen      TEXT NOT NULL CHECK (origen IN ('navegador','global','app','cli')),
  fuente      TEXT NOT NULL CHECK (fuente IN ('jira','gitlab','fs','web','texto','nota')),
  url         TEXT,
  url_norm    TEXT,
  titulo      TEXT NOT NULL,
  seleccion   TEXT,
  recurso     TEXT REFERENCES recurso(clave) ON DELETE SET NULL,
  fijada      INTEGER NOT NULL DEFAULT 0,
  capturas    INTEGER NOT NULL DEFAULT 1,
  borrada     TEXT
);
CREATE INDEX IF NOT EXISTS entrada_dia ON entrada(dia, creada);
CREATE INDEX IF NOT EXISTS entrada_recurso ON entrada(recurso);
CREATE INDEX IF NOT EXISTS entrada_url ON entrada(url_norm);
CREATE INDEX IF NOT EXISTS entrada_actualizada ON entrada(actualizada);
CREATE UNIQUE INDEX IF NOT EXISTS entrada_dia_url ON entrada(dia, url_norm)
  WHERE url_norm IS NOT NULL AND borrada IS NULL;

CREATE TABLE IF NOT EXISTS nota (
  id         INTEGER PRIMARY KEY,
  entrada_id INTEGER NOT NULL REFERENCES entrada(id) ON DELETE CASCADE,
  creada     TEXT NOT NULL,
  editada    TEXT,
  tipo       TEXT NOT NULL DEFAULT 'nota' CHECK (tipo IN ('nota','cita')),
  texto      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS nota_entrada ON nota(entrada_id, creada);

CREATE TABLE IF NOT EXISTS entidad (
  entrada_id INTEGER NOT NULL REFERENCES entrada(id) ON DELETE CASCADE,
  valor      TEXT NOT NULL,
  fuente     TEXT NOT NULL CHECK (fuente IN ('url','texto','nota')),
  PRIMARY KEY (entrada_id, valor)
);
CREATE INDEX IF NOT EXISTS entidad_valor ON entidad(valor);

CREATE TABLE IF NOT EXISTS recurso_entidad (
  clave TEXT NOT NULL REFERENCES recurso(clave) ON DELETE CASCADE,
  valor TEXT NOT NULL,
  PRIMARY KEY (clave, valor)
);
CREATE INDEX IF NOT EXISTS recurso_entidad_valor ON recurso_entidad(valor);

CREATE TABLE IF NOT EXISTS enlace_manual (
  a INTEGER NOT NULL REFERENCES entrada(id) ON DELETE CASCADE,
  b INTEGER NOT NULL REFERENCES entrada(id) ON DELETE CASCADE,
  creada TEXT NOT NULL,
  PRIMARY KEY (a, b),
  CHECK (a < b)
);

CREATE TABLE IF NOT EXISTS descarte (
  a INTEGER NOT NULL REFERENCES entrada(id) ON DELETE CASCADE,
  b INTEGER NOT NULL REFERENCES entrada(id) ON DELETE CASCADE,
  PRIMARY KEY (a, b),
  CHECK (a < b)
);

CREATE TABLE IF NOT EXISTS vocabulario (
  app    TEXT PRIMARY KEY,                  -- en minúsculas
  fuente TEXT NOT NULL                      -- bd | ajustes | repo
);

CREATE TABLE IF NOT EXISTS resumen (
  dia      TEXT PRIMARY KEY,
  texto    TEXT NOT NULL,
  generado TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS meta (
  clave TEXT PRIMARY KEY,
  valor TEXT NOT NULL
);

-- FTS: rowid = entrada.id. La mantiene Rust con reindexar(id) (borrar + insertar).
CREATE VIRTUAL TABLE IF NOT EXISTS entrada_fts USING fts5(
  titulo, url, seleccion, notas, extra,
  tokenize = "unicode61 remove_diacritics 2"
);
