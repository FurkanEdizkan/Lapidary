import fs from 'node:fs';
import path from 'node:path';
import { nanoid } from 'nanoid';
import type { FastifyInstance } from 'fastify';
import { config } from '../config.js';
import { cacheBackend, cached } from '../services/cache.service.js';
import {
  listModels, getModel, createModel, updateModel, deleteModel, getModelPaths,
  attachTag, detachTag, attachGroup, detachGroup, attachPrinter, detachPrinter,
} from '../services/model.service.js';
import { suggest, railTags } from '../services/search.service.js';
import { listTags, createTag, deleteTag } from '../services/tag.service.js';
import { listGroups, createGroup, setGroupShared, deleteGroup } from '../services/group.service.js';
import { listPins, togglePin } from '../services/pin.service.js';
import { listPrinterTypes } from '../services/printerType.service.js';
import { listSettings, addSetting, updateSetting, deleteSetting, replaceSettings } from '../services/printerSettings.service.js';
import { parseProfile } from '../services/profileImport.service.js';
import { listImages, addImage, getImagePath, deleteImage } from '../services/image.service.js';
import { ingestMesh, decompress, formatFromName } from '../services/assetPipeline.service.js';
import { scanFolder } from '../services/libraryScan.service.js';

/** Registers all /api routes. Routes stay thin; logic lives in the services. */
export async function registerApi(app: FastifyInstance): Promise<void> {
  // ---------- models ----------
  app.get('/api/models', async (req) => {
    const q = req.query as Record<string, string>;
    const key = JSON.stringify(q);
    // Short TTL: the worker writes models in a separate process, so this in-process LRU can lag; keep it small (Redis gives true cross-process invalidation).
    return cached('models', key, 2, () =>
      listModels({
        search: q.search,
        tags: q.tags ? q.tags.split(',').filter(Boolean) : undefined,
        group: q.group || undefined,
        creator: q.creator || undefined,
        sort: q.sort,
      }),
    );
  });

  app.get('/api/models/:id', async (req, reply) => {
    const { id } = req.params as { id: string };
    const model = getModel(id);
    if (!model) return reply.code(404).send({ error: 'not found' });
    return model;
  });

  app.patch('/api/models/:id', async (req, reply) => {
    const { id } = req.params as { id: string };
    const updated = updateModel(id, req.body as Record<string, unknown>);
    if (!updated) return reply.code(404).send({ error: 'not found' });
    return updated;
  });

  app.delete('/api/models/:id', async (req) => {
    const { id } = req.params as { id: string };
    return { deleted: deleteModel(id) };
  });

  // Upload a model file (+ optional metadata fields) through the asset pipeline.
  app.post('/api/models', async (req, reply) => {
    if (!req.isMultipart()) {
      // JSON path: register a metadata-only / proxy model (e.g. prototype preview shapes)
      const body = req.body as Record<string, unknown>;
      const id = `u${nanoid(10)}`;
      const model = createModel({
        id,
        name: String(body.name || 'Untitled model'),
        creator: String(body.creator || 'You'),
        type: String(body.type || 'Miniature'),
        meshKind: (body.meshKind as string) || 'egg',
        format: String(body.format || 'STL'),
        size: (body.size as [number, number, number]) || [50, 50, 50],
        tags: (body.tags as string[]) || [],
        printers: (body.printers as string[]) || [],
        settings: (body.settings as [string, string][]) || [],
      });
      return model;
    }

    const fields: Record<string, string> = {};
    let fileBuf: Buffer | null = null;
    let fileName = '';
    for await (const part of req.parts()) {
      if (part.type === 'file') {
        fileName = part.filename;
        fileBuf = await streamToBuffer(part.file);
      } else {
        fields[part.fieldname] = String(part.value);
      }
    }
    if (!fileBuf) return reply.code(400).send({ error: 'no file uploaded' });

    const id = `u${nanoid(10)}`;
    const ingest = await ingestMesh(id, fileName, fileBuf);
    const tags = (fields.tags || '').split(',').map((t) => t.trim()).filter(Boolean);
    const printers = (fields.printers || '').split(',').map((p) => p.trim()).filter(Boolean);
    const settings: [string, string][] = (fields.settings || '')
      .split('\n').map((l) => l.trim()).filter(Boolean)
      .map((line) => {
        const i = line.indexOf(':');
        return i > 0 ? [line.slice(0, i).trim(), line.slice(i + 1).trim()] : ['Note', line];
      });

    const model = createModel({
      id,
      name: fields.name || path.basename(fileName, path.extname(fileName)),
      creator: fields.creator || 'You',
      type: fields.type || 'Miniature',
      format: ingest.format || formatFromName(fileName),
      fileSizeBytes: ingest.fileSizeBytes,
      size: ingest.size ?? [
        Number(fields.sx) || 0, Number(fields.sy) || 0, Number(fields.sz) || 0,
      ],
      originalPath: ingest.originalPath,
      lodPath: ingest.lodPath,
      tags,
      printers,
      settings,
    });
    if (ingest.triangleCount) updateModel(id, { triangleCount: ingest.triangleCount });
    return model;
  });

  // ---------- asset tiers ----------
  app.get('/api/models/:id/original', async (req, reply) => {
    const { id } = req.params as { id: string };
    const paths = getModelPaths(id);
    if (!paths?.original || !fs.existsSync(paths.original)) return reply.code(404).send({ error: 'no original' });
    const buf = decompress(paths.original);
    reply.header('Content-Type', 'application/octet-stream');
    reply.header('Cache-Control', 'public, max-age=31536000, immutable');
    reply.header('X-Model-Format', paths.format);
    return reply.send(buf);
  });

  app.get('/api/models/:id/lod', async (req, reply) => {
    const { id } = req.params as { id: string };
    const paths = getModelPaths(id);
    if (!paths?.lod || !fs.existsSync(paths.lod)) return reply.code(404).send({ error: 'no lod' });
    reply.header('Content-Type', 'application/octet-stream');
    reply.header('Cache-Control', 'public, max-age=31536000, immutable');
    return reply.send(fs.readFileSync(paths.lod));
  });

  app.get('/api/models/:id/thumbnail', async (req, reply) => {
    const { id } = req.params as { id: string };
    const paths = getModelPaths(id);
    if (!paths?.thumbnail || !fs.existsSync(paths.thumbnail)) return reply.code(404).send({ error: 'no thumbnail' });
    reply.header('Content-Type', 'image/png');
    reply.header('Cache-Control', 'public, max-age=86400');
    return reply.send(fs.readFileSync(paths.thumbnail));
  });

  // Cache a client-generated thumbnail (PNG data URL) back to disk.
  app.post('/api/models/:id/thumbnail', async (req, reply) => {
    const { id } = req.params as { id: string };
    const { dataUrl } = req.body as { dataUrl: string };
    if (!dataUrl?.startsWith('data:image/png;base64,')) return reply.code(400).send({ error: 'bad data url' });
    const buf = Buffer.from(dataUrl.split(',')[1], 'base64');
    const filePath = path.join(config.thumbnailsDir, `${id}.png`);
    fs.writeFileSync(filePath, buf);
    updateModel(id, { thumbnailPath: filePath });
    return { ok: true };
  });

  // ---------- search ----------
  app.get('/api/search/suggest', async (req) => {
    const q = (req.query as { q?: string }).q || '';
    return cached('suggest', q, 3, () => suggest(q));
  });
  app.get('/api/rail-tags', async () => cached('suggest', 'rail', 3, () => railTags()));

  // ---------- scan ----------
  // Enqueues an index_archive job per library item; the worker indexes them in the
  // background. Returns { scanned, enqueued, skipped } — poll /api/models to watch rows appear.
  app.post('/api/scan', async (req, reply) => {
    const { folderPath } = req.body as { folderPath?: string };
    const target = folderPath || config.libraryPath;
    if (!target) return reply.code(400).send({ error: 'no folderPath and no LIBRARY_PATH configured' });
    if (config.libraryPath) {
      const root = path.resolve(config.libraryPath);
      const resolved = path.resolve(target);
      if (resolved !== root && !resolved.startsWith(root + path.sep)) {
        return reply.code(400).send({ error: 'folderPath must be within LIBRARY_PATH' });
      }
    }
    try {
      return scanFolder(target);
    } catch (e) {
      return reply.code(400).send({ error: (e as Error).message });
    }
  });

  // ---------- tags / groups / pins / printers ----------
  app.get('/api/tags', async () => listTags());
  app.post('/api/tags', async (req) => { createTag((req.body as { name: string }).name); return listTags(); });
  app.delete('/api/tags/:name', async (req) => { deleteTag((req.params as { name: string }).name); return listTags(); });
  app.post('/api/models/:id/tags', async (req) => {
    const { id } = req.params as { id: string };
    attachTag(id, (req.body as { name: string }).name);
    return getModel(id);
  });
  app.delete('/api/models/:id/tags/:name', async (req) => {
    const { id, name } = req.params as { id: string; name: string };
    detachTag(id, name);
    return getModel(id);
  });

  app.get('/api/groups', async () => listGroups());
  app.post('/api/groups', async (req) => { createGroup((req.body as { name: string }).name); return listGroups(); });
  app.patch('/api/groups/:name', async (req) => {
    const { name } = req.params as { name: string };
    setGroupShared(name, !!(req.body as { shared: boolean }).shared);
    return listGroups();
  });
  app.delete('/api/groups/:name', async (req) => { deleteGroup((req.params as { name: string }).name); return listGroups(); });
  app.post('/api/models/:id/groups', async (req) => {
    const { id } = req.params as { id: string };
    attachGroup(id, (req.body as { name: string }).name);
    return getModel(id);
  });
  app.delete('/api/models/:id/groups/:name', async (req) => {
    const { id, name } = req.params as { id: string; name: string };
    detachGroup(id, name);
    return getModel(id);
  });

  app.get('/api/pins', async () => listPins());
  app.post('/api/pins', async (req) => {
    const { kind, name } = req.body as { kind: string; name: string };
    togglePin(kind, name);
    return listPins();
  });

  app.get('/api/printer-types', async () => listPrinterTypes());
  app.post('/api/models/:id/printers', async (req) => {
    const { id } = req.params as { id: string };
    attachPrinter(id, (req.body as { name: string }).name);
    return getModel(id);
  });
  app.delete('/api/models/:id/printers/:name', async (req) => {
    const { id, name } = req.params as { id: string; name: string };
    detachPrinter(id, name);
    return getModel(id);
  });

  // ---------- settings ----------
  app.get('/api/models/:id/settings', async (req) => listSettings((req.params as { id: string }).id));
  app.post('/api/models/:id/settings', async (req) => {
    const { id } = req.params as { id: string };
    const { k, v } = req.body as { k: string; v: string };
    addSetting(id, k, v);
    return listSettings(id);
  });
  app.patch('/api/settings/:sid', async (req) => {
    const { sid } = req.params as { sid: string };
    const { k, v } = req.body as { k: string; v: string };
    updateSetting(Number(sid), k, v);
    return { ok: true };
  });
  app.delete('/api/settings/:sid', async (req) => {
    deleteSetting(Number((req.params as { sid: string }).sid));
    return { ok: true };
  });
  app.post('/api/models/:id/settings/import', async (req, reply) => {
    const { id } = req.params as { id: string };
    if (!req.isMultipart()) return reply.code(400).send({ error: 'expected multipart' });
    let content = '';
    let filename = 'profile.ini';
    for await (const part of req.parts()) {
      if (part.type === 'file') {
        filename = part.filename;
        content = (await streamToBuffer(part.file)).toString('utf8');
      }
    }
    const profilePath = path.join(config.profilesDir, `${id}-${nanoid(6)}-${filename}`);
    fs.writeFileSync(profilePath, content);
    const parsed = parseProfile(filename, content);
    replaceSettings(id, parsed.rows, 'profile', profilePath);
    return { imported: filename, settings: listSettings(id) };
  });

  // ---------- images ----------
  app.get('/api/models/:id/images', async (req) => listImages((req.params as { id: string }).id));
  app.post('/api/models/:id/images', async (req, reply) => {
    const { id } = req.params as { id: string };
    if (!req.isMultipart()) return reply.code(400).send({ error: 'expected multipart' });
    const fields: Record<string, string> = {};
    let buf: Buffer | null = null;
    let name = 'photo.png';
    for await (const part of req.parts()) {
      if (part.type === 'file') { name = part.filename; buf = await streamToBuffer(part.file); }
      else fields[part.fieldname] = String(part.value);
    }
    if (!buf) return reply.code(400).send({ error: 'no image' });
    return addImage(id, buf, name, fields.kind || 'printed', fields.caption);
  });
  app.get('/api/images/:iid', async (req, reply) => {
    const p = getImagePath(Number((req.params as { iid: string }).iid));
    if (!p || !fs.existsSync(p)) return reply.code(404).send({ error: 'not found' });
    return reply.type(mimeFromExt(p)).send(fs.readFileSync(p));
  });
  app.delete('/api/images/:iid', async (req) => {
    deleteImage(Number((req.params as { iid: string }).iid));
    return { ok: true };
  });

  // ---------- meta ----------
  app.get('/api/health', async () => ({ ok: true, cache: cacheBackend() }));
}

function mimeFromExt(p: string): string {
  const ext = path.extname(p).toLowerCase();
  if (ext === '.jpg' || ext === '.jpeg') return 'image/jpeg';
  if (ext === '.webp') return 'image/webp';
  if (ext === '.gif') return 'image/gif';
  return 'image/png';
}

async function streamToBuffer(stream: NodeJS.ReadableStream): Promise<Buffer> {
  const chunks: Buffer[] = [];
  for await (const chunk of stream) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks);
}
