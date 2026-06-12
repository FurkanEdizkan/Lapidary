import Database from 'better-sqlite3';
import { config } from '../config.js';
import { seedDatabase } from './seed.js';

let db: Database.Database | null = null;

/** Returns the singleton SQLite connection, creating + migrating it on first call. */
export function getDb(): Database.Database {
  if (db) return db;
  db = new Database(config.dbPath);
  db.pragma('journal_mode = WAL');
  db.pragma('foreign_keys = ON');
  migrate(db);
  seedDatabase(db);
  return db;
}

/** Idempotent schema creation, versioned via PRAGMA user_version. */
function migrate(d: Database.Database): void {
  const version = d.pragma('user_version', { simple: true }) as number;
  if (version >= 1) return;

  d.exec(`
    CREATE TABLE models (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      creator TEXT NOT NULL DEFAULT 'Unknown',
      type TEXT NOT NULL DEFAULT 'Miniature',
      mesh_kind TEXT,
      color TEXT NOT NULL DEFAULT '#bcc0c8',
      format TEXT NOT NULL DEFAULT 'STL',
      file_size_bytes INTEGER NOT NULL DEFAULT 0,
      bbox_x REAL NOT NULL DEFAULT 0,
      bbox_y REAL NOT NULL DEFAULT 0,
      bbox_z REAL NOT NULL DEFAULT 0,
      triangle_count INTEGER NOT NULL DEFAULT 0,
      created_date TEXT,
      added_date TEXT NOT NULL,
      original_path TEXT,
      lod_path TEXT,
      thumbnail_path TEXT,
      notes TEXT
    );

    CREATE TABLE tags (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL UNIQUE
    );
    CREATE TABLE model_tags (
      model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
      tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
      PRIMARY KEY (model_id, tag_id)
    );

    CREATE TABLE groups (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL UNIQUE,
      shared INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE model_groups (
      model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
      group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
      PRIMARY KEY (model_id, group_id)
    );

    CREATE TABLE printer_types (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL UNIQUE
    );
    CREATE TABLE model_printer_types (
      model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
      printer_type_id INTEGER NOT NULL REFERENCES printer_types(id) ON DELETE CASCADE,
      PRIMARY KEY (model_id, printer_type_id)
    );

    CREATE TABLE printer_settings (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
      ord INTEGER NOT NULL DEFAULT 0,
      k TEXT NOT NULL,
      v TEXT NOT NULL,
      source TEXT NOT NULL DEFAULT 'manual',
      raw_json TEXT,
      profile_path TEXT
    );

    CREATE TABLE images (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
      path TEXT NOT NULL,
      caption TEXT,
      kind TEXT NOT NULL DEFAULT 'printed'
    );

    CREATE TABLE pins (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      kind TEXT NOT NULL,
      name TEXT NOT NULL,
      UNIQUE (kind, name)
    );

    CREATE INDEX idx_model_tags_tag ON model_tags(tag_id);
    CREATE INDEX idx_model_groups_group ON model_groups(group_id);
    CREATE INDEX idx_models_creator ON models(creator);
    CREATE INDEX idx_settings_model ON printer_settings(model_id);
    CREATE INDEX idx_images_model ON images(model_id);
  `);

  d.pragma('user_version = 1');
}
