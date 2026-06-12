import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { config } from '../config.js';
import { analyzeMesh } from './meshSidecar.service.js';

/**
 * Three-tier asset pipeline. Given a raw mesh file, it:
 *   1. stores the ORIGINAL compressed (Tier 3) — zstd when available, gzip otherwise
 *   2. asks the mesh sidecar for bbox + triangle count and a decimated LOD (Tier 2)
 *   3. leaves the THUMBNAIL (Tier 1) to be generated client-side and cached back
 * Knowing a model afterwards never requires opening Tier 3.
 */
export interface IngestResult {
  originalPath: string;
  lodPath: string | null;
  fileSizeBytes: number;
  size: [number, number, number] | null;
  triangleCount: number;
  format: string;
  compression: 'zstd' | 'gzip';
}

const hasZstd = typeof (zlib as unknown as { zstdCompressSync?: unknown }).zstdCompressSync === 'function';

function compress(buf: Buffer): { data: Buffer; ext: string; codec: 'zstd' | 'gzip' } {
  if (hasZstd) {
    const z = zlib as unknown as { zstdCompressSync(b: Buffer): Buffer };
    return { data: z.zstdCompressSync(buf), ext: '.zst', codec: 'zstd' };
  }
  return { data: zlib.gzipSync(buf), ext: '.gz', codec: 'gzip' };
}

export function decompress(filePath: string): Buffer {
  const raw = fs.readFileSync(filePath);
  if (filePath.endsWith('.zst')) {
    const z = zlib as unknown as { zstdDecompressSync(b: Buffer): Buffer };
    return z.zstdDecompressSync(raw);
  }
  if (filePath.endsWith('.gz')) return zlib.gunzipSync(raw);
  return raw;
}

export function formatFromName(name: string): string {
  const ext = path.extname(name).toLowerCase().replace('.', '');
  return ext ? ext.toUpperCase() : 'STL';
}

/** Ingest a mesh buffer for `modelId`, producing Tier-3 (compressed) + Tier-2 (LOD). */
export async function ingestMesh(modelId: string, originalName: string, buffer: Buffer): Promise<IngestResult> {
  const format = formatFromName(originalName);
  const baseExt = path.extname(originalName).toLowerCase() || '.stl';

  // Tier 3 — compressed original
  const { data, ext, codec } = compress(buffer);
  const originalPath = path.join(config.modelsDir, `${modelId}${baseExt}${ext}`);
  fs.writeFileSync(originalPath, data);

  // Tier 2 — decimated LOD + metadata via the sidecar (writes a temp raw file to analyze)
  const lodPath = path.join(config.lodDir, `${modelId}.stl`);
  const tmpRaw = path.join(config.lodDir, `${modelId}.raw${baseExt}`);
  let size: [number, number, number] | null = null;
  let triangleCount = 0;
  let lodWritten = false;
  try {
    fs.writeFileSync(tmpRaw, buffer);
    const analysis = await analyzeMesh(tmpRaw, lodPath);
    if (analysis) {
      size = analysis.bbox;
      triangleCount = analysis.triangles;
      lodWritten = analysis.lodWritten;
    }
  } finally {
    try { fs.unlinkSync(tmpRaw); } catch { /* ignore */ }
  }

  return {
    originalPath,
    lodPath: lodWritten ? lodPath : null,
    fileSizeBytes: buffer.length,
    size,
    triangleCount,
    format,
    compression: codec,
  };
}
