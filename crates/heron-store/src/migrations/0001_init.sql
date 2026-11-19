-- Heron index schema, version 1.
-- Mirrors docs/IMPLEMENTATION-heron-v1.md §2.3. Migrations are append-only: never edit a
-- released file, add 0002_*.sql instead.

CREATE TABLE client (
  id            TEXT PRIMARY KEY,
  display_name  TEXT NOT NULL,
  global_paths  TEXT NOT NULL,
  project_paths TEXT NOT NULL,
  link_modes    TEXT NOT NULL,
  priority      INTEGER NOT NULL DEFAULT 0,
  native_scan   TEXT,
  verified_at   TEXT,
  verified_by   TEXT
);

CREATE TABLE skill (
  slug         TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  description  TEXT NOT NULL DEFAULT '',
  tags         TEXT NOT NULL DEFAULT '[]',
  status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','trashed')),
  origin       TEXT NOT NULL DEFAULT 'store'
               CHECK (origin IN ('store','adopted','legacy-command')),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE source_binding (
  id             INTEGER PRIMARY KEY,
  slug           TEXT NOT NULL REFERENCES skill(slug) ON DELETE CASCADE,
  kind           TEXT NOT NULL CHECK (kind IN ('git','local','adopted')),
  url            TEXT,
  ref            TEXT,
  subpath        TEXT,
  pinned_commit  TEXT,
  remote_version TEXT,
  content_hash   TEXT,
  is_primary     INTEGER NOT NULL DEFAULT 1,
  UNIQUE(slug, url, subpath)
);
CREATE INDEX idx_binding_slug ON source_binding(slug);

CREATE TABLE version (
  slug         TEXT NOT NULL REFERENCES skill(slug) ON DELETE CASCADE,
  version      TEXT NOT NULL,
  commit_sha   TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes   INTEGER NOT NULL DEFAULT 0,
  fetched_at   TEXT NOT NULL,
  origin       TEXT NOT NULL CHECK (origin IN ('upstream','local','merge')),
  PRIMARY KEY (slug, version)
);

CREATE TABLE target (
  id           TEXT PRIMARY KEY,
  kind         TEXT NOT NULL REFERENCES client(id),
  scope        TEXT NOT NULL CHECK (scope IN ('global','project')),
  project_path TEXT,
  path         TEXT NOT NULL,
  enabled      INTEGER NOT NULL DEFAULT 1,
  is_default   INTEGER NOT NULL DEFAULT 0,
  discover     TEXT NOT NULL DEFAULT 'manual' CHECK (discover IN ('auto','manual')),
  UNIQUE(kind, scope, path)
);

CREATE TABLE projection (
  id           INTEGER PRIMARY KEY,
  slug         TEXT NOT NULL REFERENCES skill(slug),
  target_id    TEXT NOT NULL REFERENCES target(id) ON DELETE CASCADE,
  link_name    TEXT NOT NULL,
  path         TEXT NOT NULL,
  mode         TEXT NOT NULL CHECK (mode IN ('copy','symlink','junction','pointer')),
  version      TEXT,
  content_hash TEXT,
  inode_kind   TEXT,
  state        TEXT NOT NULL DEFAULT 'ok'
               CHECK (state IN ('ok','broken','drifted','modified','shadowed')),
  created_at   TEXT NOT NULL,
  UNIQUE(target_id, link_name)
);
CREATE INDEX idx_projection_slug ON projection(slug);

CREATE TABLE source (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('git','local','market')),
  adapter        TEXT,
  url            TEXT,
  ref            TEXT,
  subpath        TEXT,
  path           TEXT,
  priority       INTEGER NOT NULL DEFAULT 100,
  trusted        INTEGER NOT NULL DEFAULT 0,
  enabled        INTEGER NOT NULL DEFAULT 1,
  capability     TEXT,
  last_synced_at TEXT,
  last_commit    TEXT,
  last_probe_ok  TEXT
);

CREATE TABLE detected (
  path           TEXT PRIMARY KEY,
  real_path      TEXT,
  client_id      TEXT,
  scope          TEXT,
  project_path   TEXT,
  observed_slug  TEXT,
  content_hash   TEXT,
  frontmatter_ok INTEGER,
  kind           TEXT NOT NULL DEFAULT 'skill'
                 CHECK (kind IN ('skill','legacy-command','nested-suspect')),
  last_seen_at   TEXT NOT NULL,
  adopt_state    TEXT NOT NULL DEFAULT 'candidate'
                 CHECK (adopt_state IN ('candidate','ignored','adopted','conflict')),
  mtime_ns       INTEGER,
  size_bytes     INTEGER
);
CREATE INDEX idx_detected_state ON detected(adopt_state);

CREATE TABLE visibility (
  projection_id INTEGER PRIMARY KEY REFERENCES projection(id) ON DELETE CASCADE,
  client_id     TEXT NOT NULL,
  state         TEXT NOT NULL
                CHECK (state IN ('visible','not_scanned','shadowed','restart_required','broken')),
  shadowed_by   TEXT,
  checked_at    TEXT NOT NULL
);

CREATE TABLE event (
  id        INTEGER PRIMARY KEY,
  ts        TEXT NOT NULL,
  actor     TEXT NOT NULL CHECK (actor IN ('gui','cli','system')),
  op        TEXT NOT NULL,
  subject   TEXT,
  detail    TEXT,
  undo_of   INTEGER REFERENCES event(id),
  undoable  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_event_ts ON event(ts DESC);

CREATE TABLE undo_step (
  event_id  INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  seq       INTEGER NOT NULL,
  action    TEXT NOT NULL,
  PRIMARY KEY (event_id, seq)
);

CREATE TABLE kv (
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);
