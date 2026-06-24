/**
 * Integration tests for GET /api/models/:id/original — archive-aware serving.
 *
 * Runs against the shared .test-data DB (DATA_DIR=./.test-data, set in package.json).
 */
import { beforeEach, afterEach, describe, it, expect } from 'vitest';
import path from 'node:path';
import fs from 'node:fs';
import Fastify from 'fastify';
import type { FastifyInstance } from 'fastify';
import { archivesDir, fixturesDir } from './helpers.js';
import { getDb } from '../src/db/database.js';
import { createModel, updateModel } from '../src/services/model.service.js';
import { registerApi } from '../src/routes/api.js';

const ZIP = path.join(archivesDir, 'cube.zip');
const CUBE_STL = path.join(fixturesDir, 'cube.stl');

function clean(): void {
  const d = getDb();
  for (const t of [
    'model_tags', 'model_groups', 'model_printer_types', 'printer_settings', 'images', 'jobs', 'models', 'tags', 'groups',
  ]) {
    d.prepare(`DELETE FROM ${t}`).run();
  }
}

async function buildApp(): Promise<FastifyInstance> {
  const app = Fastify({ logger: false });
  await registerApi(app);
  await app.ready();
  return app;
}

describe('GET /api/models/:id/original — archive-aware', () => {
  let app: FastifyInstance;

  beforeEach(async () => {
    clean();
    app = await buildApp();
  });

  afterEach(async () => {
    await app.close();
  });

  it('returns STL bytes (not zip bytes) for an archive model with entry_path set', async () => {
    createModel({
      id: 'arc1',
      name: 'Cube Archive',
      creator: 'test',
      type: 'model',
      format: 'STL',
      originalPath: ZIP,
    });
    updateModel('arc1', { entryPath: 'cube.stl' });

    const res = await app.inject({ method: 'GET', url: '/api/models/arc1/original' });

    expect(res.statusCode).toBe(200);

    const expectedBytes = fs.readFileSync(CUBE_STL);
    expect(res.rawPayload).toEqual(expectedBytes);
    // Must NOT be the zip itself
    const zipBytes = fs.readFileSync(ZIP);
    expect(res.rawPayload).not.toEqual(zipBytes);
  });

  it('returns 404 when archive model has no entry_path', async () => {
    createModel({
      id: 'arc2',
      name: 'Cube No Entry',
      creator: 'test',
      type: 'model',
      format: 'STL',
      originalPath: ZIP,
    });
    // No entryPath set — entry_path is NULL

    const res = await app.inject({ method: 'GET', url: '/api/models/arc2/original' });
    expect(res.statusCode).toBe(404);
    const body = JSON.parse(res.body);
    expect(body.error).toMatch(/no extractable mesh entry/);
  });

  it('returns original bytes for a non-archive (loose STL) model', async () => {
    createModel({
      id: 'stl1',
      name: 'Cube Loose',
      creator: 'test',
      type: 'model',
      format: 'STL',
      originalPath: CUBE_STL,
    });

    const res = await app.inject({ method: 'GET', url: '/api/models/stl1/original' });
    expect(res.statusCode).toBe(200);
    const expectedBytes = fs.readFileSync(CUBE_STL);
    expect(res.rawPayload).toEqual(expectedBytes);
  });
});
