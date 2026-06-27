# Model Plate Transform Editor — Phase A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user orient a real model on a build plate in the 3D viewer (move + rotate via gizmo, drop-to-plate), save the placement per model, and have it re-open in that exact orientation with the gallery thumbnail matching.

**Architecture:** Canonical placement is stored in **print-space** (Z-up, millimetres, quaternion) as a `transform_json` blob on `models`. The viewer wraps the mesh in a `pivot` group inside a `plate` group; the plate group is rotated −90° about X so Z-up content displays in the Y-up scene. Transform math lives in a pure, GL-free module so it is unit-testable (headless WebGL is unavailable in this environment). All rendered behavior reuses the existing render-on-demand, StrictMode-safe viewer handle.

**Tech Stack:** TypeScript, React 18, three.js 0.172 (`TransformControls`, `GridHelper`), Fastify, better-sqlite3, vitest.

**Spec:** `docs/superpowers/specs/2026-06-27-model-plate-transform-editor-design.md`

## Global Constraints

- three.js version is **0.172**; `TransformControls` is imported from `three/examples/jsm/controls/TransformControls.js` and added to the scene via **`control.getHelper()`** (confirmed present in 0.172).
- The viewer stays **render-on-demand** (no continuous `requestAnimationFrame` loop) and **StrictMode-safe** — the renderer/handle is created synchronously; `destroy()` must tear down everything it created.
- Canonical transform is **print-space, Z-up, millimetres**, stored as `{ position:[x,y,z], quaternion:[x,y,z,w], scale:number }`. **Persist the quaternion, never Euler** (Euler is derived only for the editing UI).
- The persisted transform is read **plate-local** (relative to the `plate` parent), not from the world/display matrix.
- `models.bbox_x/y/z` remains the **base unscaled size**; never overwrite it with scaled dimensions.
- Transform math (`dropToPlane`, quaternion compose, euler↔quat) lives in a **pure, GL-free module** (`web/src/lib/plateTransform.ts`) and is unit-tested with vitest.
- Server tests run from the `server/` workspace via `DATA_DIR=./.test-data vitest run`; test files live in `server/test/*.test.ts`.
- Design tokens (dark UI): page `#121214`, cyan accent `#2cb4f5`, fonts Archivo + JetBrains Mono. The plate must read as subtle on this background and must not z-fight the model.
- Phase A is **move + rotate + drop + persistence**. Scale, place-on-face, the full numeric panel, snapping toggles, and world/local toggle are **Phase B** — do not build them here.

---

### Task 1: DB migration v4 — `transform_json` column

**Files:**
- Modify: `server/src/db/database.ts` (add a `version < 4` block in `migrate()`)
- Test: `server/test/migrate.test.ts` (existing assertions expect v3 — update to v4 + assert the new column)

**Interfaces:**
- Consumes: existing `migrate(d: Database.Database): void`
- Produces: `models.transform_json TEXT` column; `PRAGMA user_version = 4`

- [ ] **Step 1: Update the existing migration tests to expect v4 (they will now fail)**

In `server/test/migrate.test.ts`, replace every `.toBe(3)` with `.toBe(4)`, update the first test's title to read `…and sets user_version to 4`, and add a `transform_json` assertion to the first test. The first test becomes:

```ts
  it('creates base schema + jobs + entry_path + transform_json and sets user_version to 4', () => {
    const db = new Database(':memory:');
    migrate(db);
    expect(db.pragma('user_version', { simple: true })).toBe(4);
    expect(tables(db)).toEqual(expect.arrayContaining(['models', 'tags', 'jobs']));
    const columns = (db.prepare('PRAGMA table_info(models)').all() as { name: string }[]).map((c) => c.name);
    expect(columns).toContain('transform_json');
  });
```

Also, in the `upgrades a v2 database to v3` test (the one that builds `db2`), change its final two assertions to expect v4 and `transform_json`:

```ts
    db2.pragma('user_version = 2');
    migrate(db2);
    expect(db2.pragma('user_version', { simple: true })).toBe(4);
    const columns2 = (db2.prepare('PRAGMA table_info(models)').all() as { name: string }[]).map((c) => c.name);
    expect(columns2).toContain('entry_path');
    expect(columns2).toContain('transform_json');
```

- [ ] **Step 2: Run the migration tests to verify they fail**

Run: `cd server && DATA_DIR=./.test-data npx vitest run test/migrate.test.ts`
Expected: FAIL — `expected 3 to be 4` (and missing `transform_json`).

- [ ] **Step 3: Add the v4 migration block**

In `server/src/db/database.ts`, immediately after the `if (version < 3) { … }` block inside `migrate()`, add:

```ts
  if (version < 4) {
    d.transaction(() => {
      const columns = (d.prepare('PRAGMA table_info(models)').all() as { name: string }[]).map((c) => c.name);
      if (!columns.includes('transform_json')) {
        d.exec('ALTER TABLE models ADD COLUMN transform_json TEXT');
      }
      d.pragma('user_version = 4');
    })();
    version = 4;
  }
```

- [ ] **Step 4: Run the migration tests to verify they pass**

Run: `cd server && DATA_DIR=./.test-data npx vitest run test/migrate.test.ts`
Expected: PASS (all migrate tests green).

- [ ] **Step 5: Commit**

```bash
git add server/src/db/database.ts server/test/migrate.test.ts
git commit -m "feat(db): add models.transform_json (migration v4)"
```

---

### Task 2: Persist & read the transform in the model service

**Files:**
- Modify: `server/src/services/types.ts` (add `PlateTransform`, add `transform` to `ModelDetailDTO`)
- Modify: `server/src/services/model.service.ts` (add `transform_json` to `ModelRow`; parse in `getModel`; accept `transform` in `updateModel`)
- Test: `server/test/model.transform.test.ts` (new)

**Interfaces:**
- Consumes: `createModel`, `updateModel`, `getModel` from `model.service.ts`
- Produces:
  - `PlateTransform = { position:[number,number,number]; quaternion:[number,number,number,number]; scale:number }`
  - `ModelDetailDTO.transform: PlateTransform | null`
  - `updateModel(id, { transform: PlateTransform })` persists; `getModel(id).transform` returns it.

- [ ] **Step 1: Write the failing round-trip test**

Create `server/test/model.transform.test.ts`:

```ts
import { describe, it, expect, beforeEach } from 'vitest';
import { getDb } from '../src/db/database.js';
import { createModel, updateModel, getModel } from '../src/services/model.service.js';
import type { PlateTransform } from '../src/services/types.js';

function clean(): void {
  const d = getDb();
  d.prepare('DELETE FROM models').run();
}

describe('model transform persistence', () => {
  beforeEach(() => clean());

  it('defaults transform to null', () => {
    createModel({ id: 't1', name: 'M', creator: 'C', type: 'Miniature', format: 'STL' });
    expect(getModel('t1')!.transform).toBeNull();
  });

  it('round-trips a plate transform through updateModel/getModel', () => {
    createModel({ id: 't2', name: 'M', creator: 'C', type: 'Miniature', format: 'STL' });
    const t: PlateTransform = { position: [1, 2, 3], quaternion: [0, 0, 0.7071, 0.7071], scale: 1 };
    updateModel('t2', { transform: t });
    expect(getModel('t2')!.transform).toEqual(t);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd server && DATA_DIR=./.test-data npx vitest run test/model.transform.test.ts`
Expected: FAIL — `transform` does not exist on the returned object / type error.

- [ ] **Step 3: Add the `PlateTransform` type and DTO field**

In `server/src/services/types.ts`, add above `ModelDetailDTO`:

```ts
export interface PlateTransform {
  position: [number, number, number];
  quaternion: [number, number, number, number];
  scale: number;
}
```

Then add `transform` to `ModelDetailDTO`:

```ts
export interface ModelDetailDTO extends ModelDTO {
  notes: string | null;
  settings: SettingRow[];
  images: ImageDTO[];
  transform: PlateTransform | null;
}
```

- [ ] **Step 4: Read & write `transform_json` in the model service**

In `server/src/services/model.service.ts`:

(a) Add the column to the `ModelRow` interface (after `entry_path`):

```ts
  entry_path: string | null;
  transform_json: string | null;
  notes: string | null;
```

(b) Import the type — extend the existing import:

```ts
import type { ModelDTO, ModelDetailDTO, ModelFilter, SettingRow, ImageDTO, PlateTransform } from './types.js';
```

(c) In `getModel`, set `transform` on the returned detail object. Change the final return to:

```ts
  const transform: PlateTransform | null = r.transform_json ? (JSON.parse(r.transform_json) as PlateTransform) : null;
  return { ...base, notes: r.notes, settings, images, transform };
```

(d) In `updateModel`, after the `if ('size' in patch …)` block and before `if (sets.length)`, add transform handling:

```ts
  if ('transform' in patch) {
    sets.push('transform_json = ?');
    vals.push(patch.transform ? JSON.stringify(patch.transform) : null);
  }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd server && DATA_DIR=./.test-data npx vitest run test/model.transform.test.ts`
Expected: PASS (both tests).

- [ ] **Step 6: Run the full server suite to confirm no regression**

Run: `cd server && DATA_DIR=./.test-data npx vitest run`
Expected: PASS (all suites, including migrate).

- [ ] **Step 7: Commit**

```bash
git add server/src/services/types.ts server/src/services/model.service.ts server/test/model.transform.test.ts
git commit -m "feat(api): persist & return per-model plate transform"
```

---

### Task 3: Client API + types for the transform

**Files:**
- Modify: `web/src/api/client.ts` (add `PlateTransform` type, `ModelDetail.transform`, `api.saveTransform`)

**Interfaces:**
- Consumes: existing `jsend`, `ModelDetail`
- Produces:
  - `PlateTransform` (same shape as server)
  - `ModelDetail.transform: PlateTransform | null`
  - `api.saveTransform(id: string, transform: PlateTransform): Promise<ModelDetail>`

- [ ] **Step 1: Add the type, field, and method**

In `web/src/api/client.ts`, add near the other interfaces:

```ts
export interface PlateTransform {
  position: [number, number, number];
  quaternion: [number, number, number, number];
  scale: number;
}
```

Add `transform` to `ModelDetail`:

```ts
export interface ModelDetail extends Model { notes: string | null; settings: SettingRow[]; images: ImageItem[]; transform: PlateTransform | null; }
```

Add to the `api` object (next to `patchModel`):

```ts
  saveTransform: (id: string, transform: PlateTransform) => jsend<ModelDetail>(`/api/models/${id}`, 'PATCH', { transform }),
```

- [ ] **Step 2: Typecheck the web project**

Run: `cd web && npx tsc --noEmit -p tsconfig.json`
Expected: exit 0, no errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/api/client.ts
git commit -m "feat(web): client type + saveTransform API for plate transform"
```

---

### Task 4: Add vitest to web + pure `plateTransform` math module

**Files:**
- Modify: `web/package.json` (add `vitest` devDep + `"test"` script)
- Create: `web/vitest.config.ts`
- Create: `web/src/lib/plateTransform.ts`
- Test: `web/src/lib/plateTransform.test.ts`

**Interfaces:**
- Produces (all pure, GL-free; import only `three` math classes which run in Node):
  - `PlateTransform` (re-uses the client shape; defined here for the lib)
  - `IDENTITY_TRANSFORM: PlateTransform`
  - `transformToMatrix(t): THREE.Matrix4`
  - `plateLocalMinZ(baseBox: THREE.Box3, t): number`
  - `dropToPlane(baseBox: THREE.Box3, t): PlateTransform`
  - `defaultTransform(baseBox: THREE.Box3): PlateTransform`
  - `eulerDegToQuat(x,y,z): [number,number,number,number]`
  - `quatToEulerDeg(q): [number,number,number]`

- [ ] **Step 1: Install vitest and add the script**

Run: `cd web && npm install -D vitest@^2`
Then add to `web/package.json` `scripts`:

```json
    "test": "vitest run"
```

- [ ] **Step 2: Add the vitest config (Node environment — pure math, no DOM)**

Create `web/vitest.config.ts`:

```ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
```

- [ ] **Step 3: Write the failing test**

Create `web/src/lib/plateTransform.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { Box3, Vector3 } from 'three';
import { dropToPlane, defaultTransform, plateLocalMinZ, eulerDegToQuat, quatToEulerDeg, IDENTITY_TRANSFORM } from './plateTransform';

const centeredBox = () => new Box3(new Vector3(-1, -2, -3), new Vector3(1, 2, 3));

describe('plateTransform', () => {
  it('drops the lowest point to z=0', () => {
    const t = dropToPlane(centeredBox(), IDENTITY_TRANSFORM);
    expect(plateLocalMinZ(centeredBox(), t)).toBeCloseTo(0, 6);
    expect(t.position[2]).toBeCloseTo(3, 6); // shifted up by 3 (min was -3)
  });

  it('defaultTransform centers in XY and seats on the plate', () => {
    const t = defaultTransform(centeredBox());
    expect(t.position[0]).toBe(0);
    expect(t.position[1]).toBe(0);
    expect(plateLocalMinZ(centeredBox(), t)).toBeCloseTo(0, 6);
  });

  it('round-trips euler degrees <-> quaternion', () => {
    const q = eulerDegToQuat(90, 0, 0);
    const [x, y, z] = quatToEulerDeg(q);
    expect(x).toBeCloseTo(90, 4);
    expect(y).toBeCloseTo(0, 4);
    expect(z).toBeCloseTo(0, 4);
  });
});
```

- [ ] **Step 4: Run it to verify it fails**

Run: `cd web && npx vitest run src/lib/plateTransform.test.ts`
Expected: FAIL — cannot find module `./plateTransform`.

- [ ] **Step 5: Implement the pure module**

Create `web/src/lib/plateTransform.ts`:

```ts
// Pure, GL-free transform math for the build-plate editor. Print-space: Z-up, mm.
// Imports only three's math classes (safe in Node — no WebGL/DOM).
import { Box3, Euler, Matrix4, Quaternion, Vector3 } from 'three';

export interface PlateTransform {
  position: [number, number, number];
  quaternion: [number, number, number, number];
  scale: number;
}

export const IDENTITY_TRANSFORM: PlateTransform = {
  position: [0, 0, 0], quaternion: [0, 0, 0, 1], scale: 1,
};

export function transformToMatrix(t: PlateTransform): Matrix4 {
  return new Matrix4().compose(
    new Vector3(t.position[0], t.position[1], t.position[2]),
    new Quaternion(t.quaternion[0], t.quaternion[1], t.quaternion[2], t.quaternion[3]),
    new Vector3(t.scale, t.scale, t.scale),
  );
}

/** Plate-local minimum Z of a centered mesh box `baseBox` placed under transform `t`. */
export function plateLocalMinZ(baseBox: Box3, t: PlateTransform): number {
  return baseBox.clone().applyMatrix4(transformToMatrix(t)).min.z;
}

/** New transform with Z shifted so the lowest point sits at z = 0. */
export function dropToPlane(baseBox: Box3, t: PlateTransform): PlateTransform {
  const minZ = plateLocalMinZ(baseBox, t);
  return { ...t, position: [t.position[0], t.position[1], t.position[2] - minZ] };
}

/** Default: centered over plate origin in X/Y, seated on the plate. */
export function defaultTransform(baseBox: Box3): PlateTransform {
  return dropToPlane(baseBox, { ...IDENTITY_TRANSFORM, position: [0, 0, 0] });
}

const D2R = Math.PI / 180;
const R2D = 180 / Math.PI;

export function eulerDegToQuat(xDeg: number, yDeg: number, zDeg: number): [number, number, number, number] {
  const q = new Quaternion().setFromEuler(new Euler(xDeg * D2R, yDeg * D2R, zDeg * D2R, 'XYZ'));
  return [q.x, q.y, q.z, q.w];
}

export function quatToEulerDeg(q: [number, number, number, number]): [number, number, number] {
  const e = new Euler().setFromQuaternion(new Quaternion(q[0], q[1], q[2], q[3]), 'XYZ');
  return [e.x * R2D, e.y * R2D, e.z * R2D];
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd web && npx vitest run src/lib/plateTransform.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 7: Commit**

```bash
git add web/package.json web/package-lock.json web/vitest.config.ts web/src/lib/plateTransform.ts web/src/lib/plateTransform.test.ts
git commit -m "feat(web): vitest + pure plateTransform math module"
```

---

### Task 5: Viewer — plate group, grid, Z-up display, default placement, camera framing

**Files:**
- Modify: `web/src/lib/threeViewer.ts`

**Interfaces:**
- Consumes: `PlateTransform`, `defaultTransform`, `transformToMatrix` from `./plateTransform`
- Produces: `mountViewer(host, url, format, opts?: { transform?: PlateTransform })` now renders the mesh inside a `plate`→`pivot` hierarchy on a visible plate; `destroy()` also removes the plate/grid.

- [ ] **Step 1: Add plate constants and imports**

At the top of `web/src/lib/threeViewer.ts`, extend the imports:

```ts
import { defaultTransform, transformToMatrix, type PlateTransform } from './plateTransform';
```

Add a constant near the top of the module:

```ts
const GROUND_Z = -0.02; // plate sits a hair below z=0 to avoid z-fighting (echoes OrcaSlicer GROUND_Z)
```

- [ ] **Step 2: Build the plate + pivot hierarchy and apply the placement**

In `mountViewer`, change the signature and the geometry-load block. Replace the current `loadGeometry(...).then(object => { … scene.add(object) … })` body with a plate/pivot construction. Concretely:

```ts
export function mountViewer(
  host: HTMLElement, url: string, format: string,
  opts: { transform?: PlateTransform } = {},
): ViewerHandle {
  // ... renderer/canvas/lights/camera setup unchanged ...

  // Plate group: print-space (Z-up) content, rotated into the Y-up view.
  const plate = new THREE.Group();
  plate.rotation.x = -Math.PI / 2;
  scene.add(plate);

  // Subtle plate plane + grid at z = GROUND_Z (in plate space).
  const planeMat = new THREE.MeshBasicMaterial({ color: 0x1a1a1e, transparent: true, opacity: 0.6, depthWrite: false });
  const planeMesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), planeMat); // sized after we know the model
  planeMesh.position.z = GROUND_Z;
  const grid = new THREE.GridHelper(1, 10, 0x2cb4f5, 0x2a2a30);
  (grid.material as THREE.Material).opacity = 0.35;
  (grid.material as THREE.Material).transparent = true;
  grid.rotation.x = Math.PI / 2; // GridHelper is XZ (Y-up) by default; rotate into plate's XY
  grid.position.z = GROUND_Z;
  plate.add(planeMesh, grid);

  // Pivot holds the saved transform; the mesh is offset so it is centered on the pivot origin.
  const pivot = new THREE.Group();
  plate.add(pivot);

  const baseBox = new THREE.Box3(); // centered mesh box, filled after load

  const ready = loadGeometry(url, format).then((object) => {
    if (disposed) { disposeObject(object); return; }
    const box = new THREE.Box3().setFromObject(object);
    const center = new THREE.Vector3();
    box.getCenter(center);
    object.position.sub(center);           // center the mesh on the pivot origin
    pivot.add(object);
    baseBox.copy(box).translate(center.clone().negate()); // centered base box (mm)

    // Size the plate/grid to ~1.5x the model footprint, clamped to a sensible default.
    const size = new THREE.Vector3();
    box.getSize(size);
    const footprint = Math.max(size.x, size.y, 1);
    const plateSize = Math.min(256, Math.max(80, Math.ceil(1.5 * footprint)));
    planeMesh.geometry.dispose();
    planeMesh.geometry = new THREE.PlaneGeometry(plateSize, plateSize);
    grid.scale.setScalar(plateSize); // base GridHelper is size 1

    // Apply the saved transform, or the default (centered + dropped to plate).
    const t = opts.transform ?? defaultTransform(baseBox);
    applyTransformToPivot(pivot, t);

    framed = true;
    frameCamera();
  });
```

- [ ] **Step 3: Add the helpers and the reworked camera framing**

Add module-level helpers in `web/src/lib/threeViewer.ts`:

```ts
function applyTransformToPivot(pivot: THREE.Object3D, t: PlateTransform): void {
  pivot.matrixAutoUpdate = true;
  pivot.position.set(t.position[0], t.position[1], t.position[2]);
  pivot.quaternion.set(t.quaternion[0], t.quaternion[1], t.quaternion[2], t.quaternion[3]);
  pivot.scale.setScalar(t.scale);
  pivot.updateMatrixWorld(true);
}
```

Replace the old centering/`applyCamera` framing math. The camera must frame the model **where it sits** (do not recenter the object). Add inside `mountViewer`:

```ts
  function frameCamera() {
    // World-space bbox of the placed model (not the plate), framed without moving it.
    pivot.updateMatrixWorld(true);
    const wbox = new THREE.Box3().setFromObject(pivot);
    const wcenter = new THREE.Vector3(); const wsize = new THREE.Vector3();
    wbox.getCenter(wcenter); wbox.getSize(wsize);
    radius = Math.max(wsize.x, wsize.y, wsize.z) / 2 || 1;
    dist = radius / Math.sin((camera.fov * Math.PI) / 360);
    target.copy(wcenter);
    frame();
  }
```

Update the camera state to orbit around `target` (add `const target = new THREE.Vector3();` near `state`) and change `applyCamera()` to look at `target` and offset positions by `target`:

```ts
  function applyCamera() {
    const d = dist / state.zoom;
    camera.position.set(
      target.x + d * Math.cos(state.rx) * Math.sin(state.ry),
      target.y + d * Math.sin(state.rx) + radius * 0.1,
      target.z + d * Math.cos(state.rx) * Math.cos(state.ry),
    );
    camera.lookAt(target);
    camera.updateProjectionMatrix();
  }
```

- [ ] **Step 4: Tear down the plate in `destroy()`**

In the handle's `destroy()`, before `renderer.dispose()`, dispose plate/grid resources (the existing `scene.traverse` already disposes geometries/materials, so just ensure the plate is part of the scene — it is. Add explicit grid disposal for safety):

```ts
      planeMesh.geometry.dispose(); planeMat.dispose();
      grid.geometry.dispose(); (grid.material as THREE.Material).dispose();
```

- [ ] **Step 5: Typecheck + build the web project**

Run: `cd web && npx tsc --noEmit -p tsconfig.json && npx vite build`
Expected: exit 0; bundle builds.

- [ ] **Step 6: Manual render gate (local dev with a display)**

With `npm run dev` running, open the app, open the Kabalite model, and confirm: the model sits **on** a subtle plate/grid, centered, upright-as-saved; rotating the camera orbits around the model; the browser console shows **no** `Context Lost`. (Optional automated check: reuse the headful Chrome harness from the viewer-fix session — open model, assert non-transparent pixels > 0 and no `Context Lost`.)

- [ ] **Step 7: Commit**

```bash
git add web/src/lib/threeViewer.ts
git commit -m "feat(viewer): build plate + grid, print-space pivot, reworked framing"
```

---

### Task 6: Viewer — TransformControls (move/rotate), edit API, drop-to-plate, teardown

**Files:**
- Modify: `web/src/lib/threeViewer.ts`

**Interfaces:**
- Consumes: `dropToPlane` from `./plateTransform`; the `plate`/`pivot`/`baseBox` from Task 5
- Produces, on `ViewerHandle`:
  - `setEditMode(on: boolean): void`
  - `setGizmoMode(mode: 'translate' | 'rotate'): void`
  - `getTransform(): PlateTransform` (plate-local)
  - `setTransform(t?: PlateTransform): void` (apply `t`, or the default when omitted)
  - `dropToPlane(): void`
  - `resetTransform(): void`
  - `onTransformChange(cb: (t: PlateTransform) => void): void`
  - `thumbnail(opts?: { hidePlate?: boolean }): string | null` (extends existing)

- [ ] **Step 1: Import TransformControls and add the control (hidden by default)**

Add import:

```ts
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';
```

In `mountViewer`, after the camera is created and `plate` exists, add:

```ts
  const control = new TransformControls(camera, canvas);
  control.setSpace('world');
  control.setMode('rotate');
  control.enabled = false;
  control.visible = false;
  const controlHelper = control.getHelper();
  controlHelper.visible = false;
  scene.add(controlHelper);
  let onChangeCb: ((t: PlateTransform) => void) | null = null;
  control.addEventListener('change', () => { if (framed) frame(); });
  control.addEventListener('objectChange', () => { if (onChangeCb) onChangeCb(readPivotTransform()); });
  control.addEventListener('dragging-changed', (e) => { gizmoDragging = (e as unknown as { value: boolean }).value; });
```

Add `let gizmoDragging = false;` near the other `let` state. In the camera-orbit `onDown` handler, bail while a gizmo is active: add `if (gizmoDragging) return;` as the first line of `onDown`/`onMove`.

After the mesh is added to the pivot (Task 5 Step 2, inside `ready`), attach the control:

```ts
    control.attach(pivot);
```

- [ ] **Step 2: Add the read helper and edit API to the handle**

Add inside `mountViewer`:

```ts
  function readPivotTransform(): PlateTransform {
    return {
      position: [pivot.position.x, pivot.position.y, pivot.position.z],
      quaternion: [pivot.quaternion.x, pivot.quaternion.y, pivot.quaternion.z, pivot.quaternion.w],
      scale: pivot.scale.x,
    };
  }
```

Extend the returned handle object with:

```ts
    setEditMode(on: boolean) {
      control.enabled = on; control.visible = on; controlHelper.visible = on;
      planeMesh.visible = true; grid.visible = on; // plate always visible; grid only while editing
      if (framed) frame();
    },
    setGizmoMode(mode: 'translate' | 'rotate') { control.setMode(mode); if (framed) frame(); },
    getTransform() { return readPivotTransform(); },
    setTransform(t?: PlateTransform) {
      applyTransformToPivot(pivot, t ?? defaultTransform(baseBox));
      if (onChangeCb) onChangeCb(readPivotTransform());
      if (framed) frame();
    },
    dropToPlane() {
      const next = dropToPlane(baseBox, readPivotTransform());
      pivot.position.set(next.position[0], next.position[1], next.position[2]);
      pivot.updateMatrixWorld(true);
      if (onChangeCb) onChangeCb(readPivotTransform());
      if (framed) frame();
    },
    resetTransform() {
      const next = defaultTransform(baseBox);
      applyTransformToPivot(pivot, next);
      if (onChangeCb) onChangeCb(next);
      if (framed) frame();
    },
    onTransformChange(cb) { onChangeCb = cb; },
```

Change the existing `thumbnail()` to accept the option and hide the plate while capturing:

```ts
    thumbnail(o?: { hidePlate?: boolean }) {
      if (!framed) return null;
      const hide = o?.hidePlate ?? false;
      const pv = planeMesh.visible, gv = grid.visible, cv = controlHelper.visible;
      if (hide) { planeMesh.visible = false; grid.visible = false; controlHelper.visible = false; }
      try { frame(); return canvas.toDataURL('image/png'); }
      finally { if (hide) { planeMesh.visible = pv; grid.visible = gv; controlHelper.visible = cv; frame(); } }
    },
```

Update the `ViewerHandle` interface (top of file) to declare the new members and the `thumbnail` option.

- [ ] **Step 3: Tear down the control in `destroy()`**

In `destroy()`, before disposing the renderer, add:

```ts
      control.detach();
      control.dispose();
      scene.remove(controlHelper);
```

- [ ] **Step 4: Typecheck + build**

Run: `cd web && npx tsc --noEmit -p tsconfig.json && npx vite build`
Expected: exit 0; bundle builds.

- [ ] **Step 5: Commit**

```bash
git add web/src/lib/threeViewer.ts
git commit -m "feat(viewer): move/rotate gizmo + drop-to-plate + edit API"
```

---

### Task 7: ModelViewer + TransformPanel — edit mode UI, apply saved transform, save/persist

**Files:**
- Create: `web/src/components/TransformPanel.tsx`
- Modify: `web/src/components/ModelViewer.tsx`

**Interfaces:**
- Consumes: `ViewerHandle` edit API (Task 6); `api.saveTransform`, `api.saveThumbnail`, `ModelDetail.transform` (Tasks 2/3); `PlateTransform`
- Produces: an "Edit position" mode in the viewer that saves a placement and refreshes the thumbnail.

- [ ] **Step 1: Create the minimal TransformPanel**

Create `web/src/components/TransformPanel.tsx`:

```tsx
import { C, F } from '../theme';

interface Props {
  mode: 'translate' | 'rotate';
  onMode: (m: 'translate' | 'rotate') => void;
  onDrop: () => void;
  onReset: () => void;
  onSave: () => void;
  onCancel: () => void;
  saving: boolean;
}

const btn: React.CSSProperties = {
  background: 'rgba(24,24,28,0.9)', border: `1px solid ${C.border4}`, color: '#cfcfd6',
  fontFamily: F.mono, fontSize: 10, letterSpacing: '0.1em', padding: '6px 10px', borderRadius: 6, cursor: 'pointer',
};

export function TransformPanel(p: Props) {
  const active = (on: boolean): React.CSSProperties => on ? { ...btn, borderColor: C.accent, color: C.accent } : btn;
  return (
    <div style={{ position: 'absolute', left: 14, bottom: 40, display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
      <button style={active(p.mode === 'translate')} onClick={() => p.onMode('translate')}>MOVE</button>
      <button style={active(p.mode === 'rotate')} onClick={() => p.onMode('rotate')}>ROTATE</button>
      <button style={btn} onClick={p.onDrop}>DROP TO PLATE</button>
      <button style={btn} onClick={p.onReset}>RESET</button>
      <button style={{ ...btn, borderColor: C.accent, color: C.accent }} onClick={p.onSave} disabled={p.saving}>{p.saving ? 'SAVING…' : 'SAVE'}</button>
      <button style={btn} onClick={p.onCancel}>CANCEL</button>
    </div>
  );
}
```

(If `C.accent` does not exist in `theme.ts`, use the literal `#2cb4f5` — verify the token name when implementing.)

- [ ] **Step 2: Wire edit mode into ModelViewer**

In `web/src/components/ModelViewer.tsx`:

(a) Import:

```ts
import { TransformPanel } from './TransformPanel';
```

(b) Add state near the other `useState`s:

```ts
  const [editing, setEditing] = useState(false);
  const [gizmoMode, setGizmoMode] = useState<'translate' | 'rotate'>('rotate');
  const [saving, setSaving] = useState(false);
```

(c) Pass the saved transform into `mountViewer` (real-mesh effect). Change the mount call:

```ts
    const handle = mountViewer(hostRef.current, url, model.format, { transform: model.transform ?? undefined });
```

Add `model.transform` to that effect's dependency array.

(d) Add handlers (inside the component):

```ts
  const enterEdit = () => { setEditing(true); handleRef.current?.setEditMode(true); handleRef.current?.setGizmoMode(gizmoMode); };
  const cancelEdit = () => { setEditing(false); handleRef.current?.setEditMode(false); handleRef.current?.setTransform?.(model.transform ?? undefined as never); };
  const chooseMode = (m: 'translate' | 'rotate') => { setGizmoMode(m); handleRef.current?.setGizmoMode(m); };
  const save = async () => {
    const h = handleRef.current; if (!h) return;
    setSaving(true);
    try {
      const t = h.getTransform();
      await api.saveTransform(model.id, t);
      const dataUrl = h.thumbnail({ hidePlate: true });
      if (dataUrl) await api.saveThumbnail(model.id, dataUrl).catch(() => undefined);
      setEditing(false); h.setEditMode(false);
    } finally { setSaving(false); }
  };
```

(e) Render the controls. In the returned JSX, replace the existing `RESET VIEW` button area with an "Edit position" toggle (real meshes only) and the panel:

```tsx
      {isReal && !editing && (
        <button onClick={enterEdit} className="hover-cyan" style={{ position: 'absolute', left: 14, bottom: 40, ...pill }}>EDIT POSITION</button>
      )}
      {isReal && editing && (
        <TransformPanel mode={gizmoMode} onMode={chooseMode} onDrop={() => handleRef.current?.dropToPlane()}
          onReset={() => handleRef.current?.resetTransform()} onSave={save} onCancel={cancelEdit} saving={saving} />
      )}
```

(Keep the existing `RESET VIEW` camera button where it is.)

- [ ] **Step 3: Typecheck + build**

Run: `cd web && npx tsc --noEmit -p tsconfig.json && npx vite build`
Expected: exit 0; bundle builds. (If `setTransform` isn't on the handle yet, add a thin `setTransform(t?: PlateTransform)` to the handle in Task 6 that calls `applyTransformToPivot(pivot, t ?? defaultTransform(baseBox))` — verify the signature compiles.)

- [ ] **Step 4: End-to-end persistence gate (manual, local dev with a display)**

With `npm run dev` running:
1. Open the Kabalite model → click **EDIT POSITION** → the gizmo + plate appear.
2. **ROTATE** the model with the gizmo, click **DROP TO PLATE**, click **SAVE**.
3. Confirm via API the transform persisted:
   `curl -s http://localhost:5174/api/models/<id> | node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>console.log(JSON.parse(d).transform))'`
   Expected: a non-null `{ position, quaternion, scale }`.
4. Close and reopen the model → it opens in the saved orientation; the gallery tile thumbnail reflects the pose; console shows no `Context Lost`.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/TransformPanel.tsx web/src/components/ModelViewer.tsx
git commit -m "feat(viewer): edit-position mode — move/rotate/drop, save & persist"
```

---

## Self-Review

**Spec coverage:** plate+grid (Task 5) ✓; print-space Z-up pivot + display rotation (Task 5) ✓; quaternion persistence plate-local (Tasks 2/6) ✓; move+rotate gizmo (Task 6) ✓; drop-to-plate (Tasks 4/6) ✓; default center+drop (Tasks 4/5) ✓; reworked camera framing without recentering (Task 5) ✓; destroy() teardown of controls+plate (Tasks 5/6) ✓; migration v4 + API + client (Tasks 1/2/3) ✓; posed thumbnail with plate hidden, explicit-save-wins (Tasks 6/7) ✓; pure GL-free math + tests (Task 4) ✓. **Deferred to Phase B (per spec §8):** place-on-face, scale + Size↔Scale, full numeric XYZ fields, snapping toggles (15°/90°), world/local toggle, auto-drop-after-scale timing. These are intentionally not in Phase A.

**Placeholder scan:** No TBD/TODO. Two "verify the token/ signature when implementing" notes (theme `C.accent`, handle `setTransform`) are concrete fallbacks, not deferrals.

**Type consistency:** `PlateTransform` shape identical across server `types.ts`, client `client.ts`, and lib `plateTransform.ts` (`position`/`quaternion`/`scale`). Handle methods used in Task 7 (`setEditMode`, `setGizmoMode`, `getTransform`, `dropToPlane`, `resetTransform`, `thumbnail({hidePlate})`, `setTransform`) all defined in Task 6.

## Out of scope (Phase B — separate plan)

Place-on-face, uniform scale + live Size↔Scale + uniform toggle, full numeric Pos/Rot fields, snap toggle (15°/90°, Shift), world/local rotate toggle, auto-drop-after-scale rule, server-side transform-aware thumbnails, per-printer bed sizes, auto-orient, G-code/slicing.
