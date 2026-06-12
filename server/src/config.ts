import path from 'node:path';
import fs from 'node:fs';

/**
 * Central runtime configuration, resolved once from the environment.
 * All paths are absolute so services never depend on the process cwd.
 */
export interface Config {
  port: number;
  dataDir: string;
  modelsDir: string;
  lodDir: string;
  thumbnailsDir: string;
  imagesDir: string;
  profilesDir: string;
  dbPath: string;
  redisUrl: string | null;
  libraryPath: string | null;
  meshSidecarBin: string | null;
  webDist: string | null;
}

function resolveDataDir(): string {
  const raw = process.env.DATA_DIR || './data';
  return path.resolve(raw);
}

function ensureDir(dir: string): string {
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function resolveWebDist(): string | null {
  // In production the built SPA is served by Fastify. Look a couple of likely spots.
  const candidates = [
    path.resolve(process.cwd(), 'web/dist'),
    path.resolve(import.meta.dirname, '../../web/dist'),
    path.resolve(import.meta.dirname, '../../../web/dist'),
  ];
  for (const c of candidates) {
    if (fs.existsSync(path.join(c, 'index.html'))) return c;
  }
  return null;
}

export function loadConfig(): Config {
  const dataDir = ensureDir(resolveDataDir());
  return {
    port: Number(process.env.PORT || 5174),
    dataDir,
    modelsDir: ensureDir(path.join(dataDir, 'models')),
    lodDir: ensureDir(path.join(dataDir, 'lod')),
    thumbnailsDir: ensureDir(path.join(dataDir, 'thumbnails')),
    imagesDir: ensureDir(path.join(dataDir, 'images')),
    profilesDir: ensureDir(path.join(dataDir, 'profiles')),
    dbPath: path.join(dataDir, 'manifold.db'),
    redisUrl: process.env.REDIS_URL || null,
    libraryPath: process.env.LIBRARY_PATH || null,
    meshSidecarBin: process.env.MESH_SIDECAR_BIN || null,
    webDist: resolveWebDist(),
  };
}

export const config = loadConfig();
