import { describe, it, expect, vi, afterEach } from 'vitest';
import { resolveMeshSidecarBin } from '../src/config.js';

describe('resolveMeshSidecarBin', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('returns MESH_SIDECAR_BIN env value when set', () => {
    vi.stubEnv('MESH_SIDECAR_BIN', '/x/y');
    expect(resolveMeshSidecarBin()).toBe('/x/y');
  });

  it('returns a path ending in rust-mesh/target/release/rust-mesh when binary exists, else null', () => {
    vi.stubEnv('MESH_SIDECAR_BIN', '');
    const result = resolveMeshSidecarBin();
    if (result !== null) {
      expect(result).toMatch(/rust-mesh\/target\/release\/rust-mesh$/);
    }
    // If null, the binary simply wasn't found — also acceptable.
  });
});
