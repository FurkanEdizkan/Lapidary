/**
 * Integration test for meshSidecar.renderAndAnalyze.
 * Requires the rust-mesh binary at rust-mesh/target/release/rust-mesh.
 * Skipped automatically in CI without a Rust build.
 */
import { describe, it, expect, afterAll } from 'vitest';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');
const binPath = path.join(repoRoot, 'rust-mesh/target/release/rust-mesh');
const fixturesDir = path.join(repoRoot, 'fixtures');
const cubeStl = path.join(fixturesDir, 'cube.stl');

// Temp files cleaned up in afterAll
const tempFiles: string[] = [];

function makeTempPath(suffix: string): string {
  const p = path.join(os.tmpdir(), `lap-render-test-${Date.now()}-${Math.random().toString(36).slice(2)}${suffix}`);
  tempFiles.push(p);
  return p;
}

afterAll(() => {
  for (const f of tempFiles) {
    try { fs.rmSync(f, { force: true }); } catch { /* ignore */ }
  }
});

// Skip the entire suite when the binary is not built (CI without Rust).
describe.skipIf(!fs.existsSync(binPath))('meshSidecar.renderAndAnalyze (integration)', () => {
  it('returns bbox, triangles, and writes LOD + PNG thumbnail', async () => {
    // The config module auto-detects the binary from disk, so sidecarAvailable()
    // will return true when the binary exists. Import after any env setup.
    const { renderAndAnalyze } = await import('../src/services/meshSidecar.service.js');

    const lodOut = makeTempPath('.stl');
    const thumbOut = makeTempPath('.png');

    const result = await renderAndAnalyze(cubeStl, lodOut, thumbOut, 128);

    expect(result).not.toBeNull();
    expect(result!.bbox).toHaveLength(3);
    // cube.stl is a 20×20×20 mm cube
    expect(result!.bbox[0]).toBeCloseTo(20, 0);
    expect(result!.bbox[1]).toBeCloseTo(20, 0);
    expect(result!.bbox[2]).toBeCloseTo(20, 0);
    expect(result!.triangles).toBe(12);
    expect(result!.lodWritten).toBe(true);
    expect(result!.thumbWritten).toBe(true);

    // Verify LOD and thumb files actually exist on disk
    expect(fs.existsSync(lodOut)).toBe(true);
    expect(fs.existsSync(thumbOut)).toBe(true);

    // Verify thumb starts with PNG magic bytes: 89 50 4E 47 0D 0A 1A 0A
    const buf = fs.readFileSync(thumbOut);
    expect(buf[0]).toBe(0x89);
    expect(buf[1]).toBe(0x50); // P
    expect(buf[2]).toBe(0x4e); // N
    expect(buf[3]).toBe(0x47); // G
  });

  it('returns null when the binary path is invalid (graceful degradation)', async () => {
    const { renderAndAnalyze } = await import('../src/services/meshSidecar.service.js');
    // Call with a non-existent input: the binary should error, we get null back
    const result = await renderAndAnalyze('/does/not/exist.stl', makeTempPath('.stl'), makeTempPath('.png'), 64);
    expect(result).toBeNull();
  });
});
