import path from 'node:path';
import fs from 'node:fs';
import Fastify from 'fastify';
import multipart from '@fastify/multipart';
import fastifyStatic from '@fastify/static';
import { config } from './config.js';
import { getDb } from './db/database.js';
import { initCache, cacheBackend } from './services/cache.service.js';
import { registerApi } from './routes/api.js';

async function main(): Promise<void> {
  const app = Fastify({ logger: { level: process.env.LOG_LEVEL || 'info' }, bodyLimit: 512 * 1024 * 1024 });

  await app.register(multipart, { limits: { fileSize: 512 * 1024 * 1024 } });

  // Initialize DB (runs migrations + seed) and cache before serving.
  getDb();
  await initCache();

  await registerApi(app);

  // Serve the built SPA in production; SPA fallback to index.html for client routes.
  if (config.webDist) {
    await app.register(fastifyStatic, { root: config.webDist, prefix: '/' });
    app.setNotFoundHandler((req, reply) => {
      if (req.url.startsWith('/api/')) return reply.code(404).send({ error: 'not found' });
      return reply.sendFile('index.html');
    });
  }

  await app.listen({ port: config.port, host: '0.0.0.0' });
  app.log.info(
    `Manifold Print Library on :${config.port} | data=${config.dataDir} | cache=${cacheBackend()} | spa=${config.webDist ? 'served' : 'dev (vite)'}`,
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

// Ensure the data dir tree exists (config already creates it, but be explicit for clarity).
fs.mkdirSync(path.dirname(config.dbPath), { recursive: true });
