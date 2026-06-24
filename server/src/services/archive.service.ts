import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { promises as fsp } from 'node:fs';
import AdmZip from 'adm-zip';
import sevenBin from '7zip-bin';
import { createExtractorFromFile } from 'node-unrar-js';

const execFileP = promisify(execFile);

let sevenZipReady = false;
async function ensureSevenZipExecutable(): Promise<void> {
  if (sevenZipReady) return;
  // 7zip-bin sometimes ships without the exec bit (and npm/Docker can drop it);
  // chmod is best-effort and a harmless no-op on Windows.
  try { await fsp.chmod(sevenBin.path7za, 0o755); } catch { /* ignore */ }
  sevenZipReady = true;
}

export interface MeshEntry {
  innerPath: string;
  ext: string;
  sizeBytes: number;
}

export const MESH_EXTS = new Set(['.stl', '.3mf', '.obj']);

export type ArchiveReader = (archivePath: string) => Promise<MeshEntry[]>;

/** List supported mesh entries inside a .zip (pure JS, no external binary). */
export async function listZip(archivePath: string): Promise<MeshEntry[]> {
  const zip = new AdmZip(archivePath);
  return zip
    .getEntries()
    .filter((e) => !e.isDirectory)
    .map((e) => ({
      innerPath: e.entryName,
      ext: path.extname(e.entryName).toLowerCase(),
      sizeBytes: e.header.size,
    }))
    .filter((e) => MESH_EXTS.has(e.ext));
}

/**
 * List supported mesh entries inside a .7z by shelling out to the bundled 7za with a
 * technical listing (`-slt`): blocks of "Path = …" / "Size = …" / "Attributes = …"
 * separated by blank lines. Avoids any node-7z stream/type quirks.
 */
export async function listSevenZip(archivePath: string): Promise<MeshEntry[]> {
  await ensureSevenZipExecutable();
  const { stdout } = await execFileP(sevenBin.path7za, ['l', '-slt', archivePath], {
    maxBuffer: 64 * 1024 * 1024,
  });
  const entries: MeshEntry[] = [];
  for (const block of stdout.split(/\r?\n\r?\n/)) {
    const pathMatch = block.match(/^Path = (.+)$/m);
    if (!pathMatch) continue;
    const innerPath = pathMatch[1].trim();
    const attrs = (block.match(/^Attributes = (.+)$/m)?.[1] ?? '').trim();
    if (attrs.startsWith('D')) continue; // directory entry
    const ext = path.extname(innerPath).toLowerCase();
    if (!MESH_EXTS.has(ext)) continue;
    const sizeBytes = Number(block.match(/^Size = (\d+)$/m)?.[1] ?? 0);
    entries.push({ innerPath, ext, sizeBytes });
  }
  return entries;
}

/** List supported mesh entries inside a .rar (pure-WASM, no system binary). */
export async function listRar(archivePath: string): Promise<MeshEntry[]> {
  const extractor = await createExtractorFromFile({ filepath: archivePath });
  const list = extractor.getFileList();
  const entries: MeshEntry[] = [];
  for (const h of list.fileHeaders) {
    if (h.flags.directory) continue;
    const ext = path.extname(h.name).toLowerCase();
    if (!MESH_EXTS.has(ext)) continue;
    const sizeBytes = Number((h as { unpSize?: number }).unpSize ?? 0) || 0; // unpacked size
    entries.push({ innerPath: h.name, ext, sizeBytes });
  }
  return entries;
}

const DEFAULT_READERS: Record<string, ArchiveReader> = {
  '.zip': listZip,
  '.7z': listSevenZip,
  '.rar': listRar,
};

/** List mesh entries inside an archive, dispatching on file extension. */
export async function listMeshEntries(
  archivePath: string,
  readers: Record<string, ArchiveReader> = DEFAULT_READERS,
): Promise<MeshEntry[]> {
  const ext = path.extname(archivePath).toLowerCase();
  const reader = readers[ext];
  if (!reader) throw new Error(`Unsupported archive type: ${ext}`);
  return reader(archivePath);
}
