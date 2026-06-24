import { describe, it, expect } from 'vitest';
import Database from 'better-sqlite3';
import { migrate } from '../src/db/database.js';

function tables(db: Database.Database): string[] {
  return (db.prepare("SELECT name FROM sqlite_master WHERE type='table'").all() as { name: string }[])
    .map((r) => r.name);
}

describe('migrate', () => {
  it('creates base schema + jobs and sets user_version to 2', () => {
    const db = new Database(':memory:');
    migrate(db);
    expect(db.pragma('user_version', { simple: true })).toBe(2);
    expect(tables(db)).toEqual(expect.arrayContaining(['models', 'tags', 'jobs']));
  });

  it('upgrades a v1 database to v2 without recreating existing tables', () => {
    const db = new Database(':memory:');
    migrate(db);                 // -> v2
    db.exec('DROP TABLE jobs');  // simulate a pre-jobs v1 database
    db.pragma('user_version = 1');
    migrate(db);                 // should only add jobs, bump to 2
    expect(db.pragma('user_version', { simple: true })).toBe(2);
    expect(tables(db)).toContain('jobs');
  });
});
