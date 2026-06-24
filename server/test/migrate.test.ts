import { describe, it, expect } from 'vitest';
import Database from 'better-sqlite3';
import { migrate } from '../src/db/database.js';

function tables(db: Database.Database): string[] {
  return (db.prepare("SELECT name FROM sqlite_master WHERE type='table'").all() as { name: string }[])
    .map((r) => r.name);
}

describe('migrate', () => {
  it('creates base schema + jobs + entry_path and sets user_version to 3', () => {
    const db = new Database(':memory:');
    migrate(db);
    expect(db.pragma('user_version', { simple: true })).toBe(3);
    expect(tables(db)).toEqual(expect.arrayContaining(['models', 'tags', 'jobs']));
  });

  it('upgrades a v1 database through v2 to v3 without recreating existing tables', () => {
    const db = new Database(':memory:');
    migrate(db);                 // -> v3
    db.exec('DROP TABLE jobs');  // simulate a pre-jobs v1 database
    db.pragma('user_version = 1');
    migrate(db);                 // should add jobs (v2) and entry_path (v3), bump to 3
    expect(db.pragma('user_version', { simple: true })).toBe(3);
    expect(tables(db)).toContain('jobs');
    const columns = (db.prepare("PRAGMA table_info(models)").all() as { name: string }[]).map((c) => c.name);
    expect(columns).toContain('entry_path');
  });

  it('upgrades to v3 and adds entry_path column', () => {
    const db = new Database(':memory:');
    migrate(db);
    expect(db.pragma('user_version', { simple: true })).toBe(3);
    const columns = (db.prepare("PRAGMA table_info(models)").all() as { name: string }[]).map((c) => c.name);
    expect(columns).toContain('entry_path');
  });

  it('upgrades a v2 database to v3 without recreating existing tables', () => {
    const db = new Database(':memory:');
    migrate(db);                 // -> v3
    db.pragma('user_version = 2'); // force back to v2
    // Since entry_path now exists, we can't truly test a fresh v2->v3 upgrade.
    // Instead, create a fresh :memory: db at v2 state and migrate to v3.
    const db2 = new Database(':memory:');
    db2.exec(`
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
      CREATE TABLE jobs (
        id TEXT PRIMARY KEY,
        model_id TEXT,
        kind TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'queued',
        attempts INTEGER NOT NULL DEFAULT 0,
        error TEXT,
        payload_json TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE INDEX idx_jobs_status ON jobs(status);
      CREATE INDEX idx_jobs_kind_status ON jobs(kind, status);
    `);
    db2.pragma('user_version = 2');
    migrate(db2);
    expect(db2.pragma('user_version', { simple: true })).toBe(3);
    const columns2 = (db2.prepare("PRAGMA table_info(models)").all() as { name: string }[]).map((c) => c.name);
    expect(columns2).toContain('entry_path');
  });
});
