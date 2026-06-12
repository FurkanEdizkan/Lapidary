import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs';
import { config } from '../config.js';

const execFileAsync = promisify(execFile);

/**
 * Thin wrapper around the optional Rust `rust-mesh` binary. It computes bbox +
 * triangle count and (optionally) writes a decimated LOD mesh. Everything here
 * degrades gracefully: if the binary is missing or errors, callers get null and
 * fall back to the client-side path.
 */
export interface MeshAnalysis {
  bbox: [number, number, number];
  triangles: number;
  lodWritten: boolean;
}

let availability: boolean | null = null;

export async function sidecarAvailable(): Promise<boolean> {
  if (availability != null) return availability;
  if (!config.meshSidecarBin) {
    availability = false;
    return false;
  }
  try {
    await execFileAsync(config.meshSidecarBin, ['--version'], { timeout: 5000 });
    availability = true;
  } catch {
    availability = false;
  }
  return availability;
}

/** Analyze a mesh file, writing a decimated LOD to `lodOut` when possible. */
export async function analyzeMesh(inputPath: string, lodOut: string): Promise<MeshAnalysis | null> {
  if (!(await sidecarAvailable()) || !config.meshSidecarBin) return null;
  try {
    const { stdout } = await execFileAsync(
      config.meshSidecarBin,
      [inputPath, '--lod', lodOut, '--json'],
      { timeout: 60000, maxBuffer: 1024 * 1024 },
    );
    const parsed = JSON.parse(stdout) as { bbox: [number, number, number]; triangles: number };
    return {
      bbox: parsed.bbox,
      triangles: parsed.triangles,
      lodWritten: fs.existsSync(lodOut),
    };
  } catch {
    return null;
  }
}
