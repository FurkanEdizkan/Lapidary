import path from 'node:path';
import { fileURLToPath } from 'node:url';

// fileURLToPath(import.meta.url) is the portable way to get this file's dir under
// both Node and Vitest. server/test -> ../../ is the repo root.
const here = path.dirname(fileURLToPath(import.meta.url));

export const fixturesDir = path.resolve(here, '../../fixtures');
export const archivesDir = path.resolve(fixturesDir, 'archives');
