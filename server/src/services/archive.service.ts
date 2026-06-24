import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs';
import { promises as fsp } from 'node:fs';
import AdmZip from 'adm-zip';
import sevenBin from '7zip-bin';
import { createExtractorFromFile } from 'node-unrar-js';

const execFileP = promisify(execFile);

let sevenZipReadyP: Promise<void> | null = null;
function ensureSevenZipExecutable(): Promise<void> {
  // chmod once per process; cache the promise so concurrent first-callers share it.
  // best-effort: harmless no-op on Windows / read-only FS.
  sevenZipReadyP ??= (async () => {
    try { await fsp.chmod(sevenBin.path7za, 0o755); } catch { /* ignore */ }
  })();
  return sevenZipReadyP;
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
    timeout: 60_000,
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

/**
 * Extract a single entry flat (no sub-directories) from an archive into destDir.
 * Returns the path of the written file: `path.join(destDir, path.basename(innerPath))`.
 */
export async function extractEntry(
  archivePath: string,
  innerPath: string,
  destDir: string,
): Promise<string> {
  const ext = path.extname(archivePath).toLowerCase();
  const out = path.join(destDir, path.basename(innerPath));

  if (ext === '.zip') {
    const zip = new AdmZip(archivePath);
    // Try direct lookup first; fall back to scanning entries by name.
    let buf: Buffer | null = zip.readFile(innerPath);
    if (!buf) {
      const entry = zip.getEntries().find((e) => e.entryName === innerPath);
      if (!entry) throw new Error(`Entry not found in zip: ${innerPath}`);
      buf = zip.readFile(entry);
    }
    if (!buf) throw new Error(`Failed to read entry from zip: ${innerPath}`);
    fs.writeFileSync(out, buf);
    return out;
  }

  if (ext === '.7z') {
    await ensureSevenZipExecutable();
    await execFileP(
      sevenBin.path7za,
      ['e', archivePath, `-o${destDir}`, innerPath, '-y'],
      { timeout: 120_000, maxBuffer: 64 * 1024 * 1024 },
    );
    return out;
  }

  if (ext === '.rar') {
    // createExtractorFromFile writes files to targetPath.
    // Use basename filenameTransform so extraction is flat (no sub-dirs).
    const extractor = await createExtractorFromFile({
      filepath: archivePath,
      targetPath: destDir,
      filenameTransform: (filename: string) => path.basename(filename),
    });
    const extracted = extractor.extract({ files: [innerPath] });
    // Consume the generator to trigger the actual file writes.
    for (const _file of extracted.files) { /* write happens as a side-effect */ }
    return out;
  }

  throw new Error(`Unsupported archive type: ${ext}`);
}
