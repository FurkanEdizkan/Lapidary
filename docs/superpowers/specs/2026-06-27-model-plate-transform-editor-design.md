# Model Plate Transform Editor — Design Spec

- **Date:** 2026-06-27
- **Status:** Approved (brainstorming) — pending user spec review → implementation plan
- **Branch context:** stacks on the StrictMode viewer fix (`web/src/lib/threeViewer.ts`,
  `web/src/components/ModelViewer.tsx`), currently uncommitted in the working tree.

## 1. Goal

Let a user place each real model on a build **plate** in the 3D viewer — rotate it, move it,
scale it, seat it flat — and **save that placement per model** so the model always re-opens in
the same orientation. The saved placement is a **real print orientation** (Z-up, millimetres),
reusable later for print/export, and it also drives the gallery thumbnail so the library looks
consistent.

This is the "Both" purpose from brainstorming: canonical orientation used for display now and
meaningful for printing later.

## 2. Non-goals (v1 scope guardrails / YAGNI)

Explicitly **out** of v1 (noted as future, not built):

- Per-axis (non-uniform) scale — v1 is **uniform scale only**.
- Per-printer bed-size presets — the plate auto-sizes to the model.
- Server-side, transform-aware thumbnail regeneration in the rust-mesh worker.
- Auto-orient ("best print orientation" search — OrcaSlicer's `Orient`/Tweaker algorithm).
- Multi-object plates / arrange.
- **Any G-code / slicing.** That is the entire slicer; a gallery's placement editor stops at
  producing a correct oriented transform.

## 3. User-facing behavior

- Real-mesh viewer (the `isReal` Three.js path) gains an **"Edit position"** toggle. Proxy/seeded
  models (2D renderer) are unaffected.
- Entering edit mode shows: the **plate** (subtle plane + grid), a transform **gizmo**, and a
  **control panel**.
- Controls:
  - **Mode:** Move · Rotate · Scale (gizmo + numeric, two-way synced).
  - **Place on face:** click any face of the model → it rotates so that face lies flat on the
    plate, then re-seats.
  - **Drop to plate:** seats the current lowest point at z = 0.
  - **Numeric fields:** Pos X/Y/Z (mm), Rot X/Y/Z (°), Scale × with **live Size (mm)**, plus a
    uniform-scale toggle, a world/local rotate toggle (**default: world**, per OrcaSlicer), and a
    snap toggle (15° / 90°).
  - **Reset to original** (clears the saved transform → raw file orientation).
  - **Save** / **Cancel**.
- On **Save**, the placement persists and the gallery thumbnail updates to match.
- Snapping (Shift-to-snap, OrcaSlicer-derived): rotate **15°** (90° via toggle), move **1 mm**,
  scale **5%**.

## 4. Architecture

### 4.1 Coordinate frames

Canonical data lives in **print-space**: right-handed, **Z-up**, millimetres, plate surface at
**z = 0**, origin at **plate center**. (Mirrors OrcaSlicer `Model.cpp` `ensure_on_bed` /
`min_z`, and 3MF conventions.)

The viewer is Y-up, so all print-space content lives inside a **`plate` group rotated −90° about
X** for display. Everything below — plane/grid, model pivot, drop-to-plate, place-on-face, bbox
math — is expressed in the plate's local Z-up frame; only the `plate` group carries the display
rotation.

```
scene (Y-up, view)
└─ plate group  (rotateX -90° → presents Z-up content)   [local frame: X right, Y depth, Z up]
   ├─ ground plane + grid  (at z = GROUND_Z ≈ -0.02 to avoid z-fighting)
   └─ pivot group          (the saved transform: position, quaternion, scale)
      └─ mesh              (offset so the model is centered on the pivot origin at identity)
```

### 4.2 Transform data model

Persisted per model, **plate-local** (read relative to the `plate` parent, NOT the world/display
matrix):

```jsonc
{ "position": [x, y, z],          // mm, plate-space
  "quaternion": [x, y, z, w],     // rotation — quaternion, NOT Euler
  "scale": 1.0 }                  // uniform
```

Rationale (from OrcaSlicer `Geometry.cpp`): Euler angles are derived from the matrix
non-uniquely (gimbal lock), so we persist a **quaternion**. The numeric Rot X/Y/Z° fields are an
**editing convenience only** — converted euler→quaternion on input, quaternion→euler for display.

Composition is `T · R · S` (three.js `Matrix4.compose` order — matches Orca).

### 4.3 Pure transform math (GL-free, testable)

A standalone module (e.g. `web/src/lib/plateTransform.ts`) holds the math as **pure functions
over plain numbers / `THREE.Vector3` / `THREE.Quaternion`** — no renderer, no GL context:

- `dropToPlane(meshLocalBox, currentPos) → newPosZ` — subtract the transformed mesh's **min local
  Z** so the lowest point reaches z = 0.
- `placeOnFace(faceNormalPlateSpace) → quaternion` — `setFromUnitVectors(normal, (0,0,-1))`
  (down in plate-space), composed onto the current rotation.
- `composeTransform` / `decompose` / quaternion↔euler helpers.
- `sizeFromScale(baseSize, scale)` and `scaleFromSize(baseSize, newSize)` for the Size↔Scale
  linkage.

**Why pure:** headless WebGL is unavailable in this environment (proven: swiftshader context-loss,
EGL `GL_VENDOR=Disabled`; only headful Chrome renders). Pure functions test in CI; rendered
behavior is gated by the headful harness (§7).

### 4.4 Viewer changes (`threeViewer.ts`)

Extends the existing **render-on-demand**, StrictMode-safe handle (no continuous rAF loop):

- Build the `plate` group (plane + `GridHelper`) and `pivot` group; apply the saved transform on
  load (or default = centered + dropped to plate when none saved).
- Add `TransformControls` (`three/examples/jsm/controls/TransformControls.js`) attached to the
  pivot. Modes translate/rotate/scale; `setRotationSnap`/`setTranslationSnap`/`setScaleSnap` (snap
  gated by Shift in the UI). Re-render on its `change` event.
- **Gizmo vs camera orbit:** on `dragging-changed`, suppress the camera-orbit pointer handlers
  while a gizmo handle is active; empty-space drag still orbits.
- **Place on face:** `Raycaster.intersectObject(mesh)` → take the hit triangle normal, transform
  by the **inverse-transpose normal matrix** (`new THREE.Matrix3().getNormalMatrix(mesh.matrixWorld)`)
  into plate-space, call `placeOnFace`, then drop. (No convex-hull face clustering in v1 — the
  single hit-triangle normal suffices for flat faces.)
- **Camera framing — reworked, not reused.** The StrictMode fix centered the single object at the
  origin; this feature positions the model **freely** on the plate, so framing must **frame the
  model's current world bbox WITHOUT moving it**. The StrictMode-safe lifecycle is preserved; only
  the framing math changes.
- **`destroy()` must also tear down** `TransformControls` (remove its helper from the scene, drop
  its listeners, dispose geometries/materials) **and** the plane/grid. Missing this regresses the
  leak-free / context-safe property the fix just won.
- Handle API additions: `setEditMode(bool)`, `setGizmoMode('translate'|'rotate'|'scale')`,
  `getTransform()` (plate-local), `setTransform(T)`, `placeOnFaceAt(pointer)`, `dropToPlane()`,
  `resetTransform()`, `setUniformScale(bool)`, `setRotateSpace('world'|'local')`,
  `onTransformChange(cb)`, `thumbnail({ hidePlate: true })`.

### 4.5 Auto-drop timing rule

After **rotate** or **scale**, auto re-seat on the plate (drop). **Do NOT** auto-drop while the
user is deliberately setting vertical position (Z field, or the Move gizmo's vertical axis) — or
manual height fights the auto-seat. (OrcaSlicer's own rule: scale pivots on the bottom plane;
drop-to-bed is otherwise explicit.)

### 4.6 UI (`ModelViewer.tsx` + new `TransformPanel.tsx`)

- "Edit position" toggle (real meshes only) → reveals gizmo + plate + `TransformPanel`.
- `TransformPanel`: mode buttons, numeric fields (synced to the gizmo via `onTransformChange`),
  Place-on-face / Drop-to-plate / Reset / snap / uniform / world-local controls, Save / Cancel.
- Follows the project design system (dark `#121214`, cyan `#2cb4f5`, Archivo + JetBrains Mono).
- Editing **Scale** updates the live **Size (mm)**; editing **Size** back-solves scale.

### 4.7 Size / scale data model

- `models.bbox_x/y/z` remains the **BASE unscaled size** (never overwritten with scaled dims —
  it's the reference for the Size↔Scale linkage).
- `scale` is stored separately in `transform_json`.
- The **Dimensions** spec in the detail panel renders the **scaled** size, **computed** as
  `base × scale` (a print feature should show the real print dimensions).

### 4.8 Persistence (DB + API)

- **Migration v4:** `ALTER TABLE models ADD COLUMN transform_json TEXT` (idempotent, guarded like
  v3's `entry_path` check; bump `user_version` to 4).
- `getModel` parses `transform_json` → `transform` field on the DTO (`null` when unset).
- Extend `updateModel`'s allowlist to accept `transform` → serialize to `transform_json`.
- Client: `ModelDetail.transform` type; `api.saveTransform(id, T)` (PATCH).

### 4.9 Thumbnail capture

- On **Save**, capture the canvas with the **plate/grid hidden** (`thumbnail({ hidePlate: true })`)
  so no grid is baked into gallery tiles.
- **Reconcile the two save paths:** the StrictMode fix already auto-saves a thumbnail on first
  render when a model lacks one. Rule: the **explicit Save (posed) thumbnail always wins**; the
  first-render auto-save only applies when there is no saved transform and no thumbnail yet.

## 5. Data flow

1. Open model → `getModel` returns `transform` → viewer builds plate/pivot, applies it (or default
   center+drop) → render.
2. Edit mode → gizmo + panel manipulate the pivot → `onTransformChange` keeps numeric fields and
   the live Size in sync; rotate/scale auto-drop per §4.5.
3. Save → read **plate-local** `getTransform()` → `api.saveTransform` (PATCH → `transform_json`) →
   capture plate-hidden thumbnail → `saveThumbnail` → invalidate `model`/`models` queries.
4. Reopen → identical pose.

## 6. Error handling / edge cases

- **Degenerate/ tiny meshes** (e.g. cube): bbox min = max guarded; default `radius = 1`.
- **Place-on-face miss** (ray hits nothing): no-op with a subtle hint.
- **Non-uniform implications:** uniform scale only, so normals stay valid; still use the normal
  matrix for place-on-face robustness.
- **Save failure:** keep edit mode open, surface an inline error, do not lose the in-editor pose.
- **Model with no saved transform:** default = centered horizontally + dropped to plate.

## 7. Testing strategy

- **Server (vitest):** `transform_json` round-trip through `updateModel`/`getModel`; v4 migration
  applies and is idempotent.
- **Viewer math (vitest, pure functions):** `dropToPlane`, `placeOnFace` (normal→quaternion),
  quaternion↔euler round-trip, `sizeFromScale`/`scaleFromSize`. No GL context required.
- **Manual gate (headful Chrome harness):** open model → rotate/scale/place-on-face/drop → Save →
  reopen → assert the transform persisted and the render matches; assert no `Context Lost` and no
  GPU/context leak across StrictMode remounts.

## 8. Implementation phasing (for the plan)

The work is large for one plan; phase it (same approved scope):

- **Phase A — foundation:** plate + grid, pivot transform model + pure math module, **Move +
  Rotate** gizmo, **Drop to plate**, persistence (migration v4 + API + client), reworked camera
  framing, `destroy()` teardown, posed-thumbnail save. Ship a usable orient-and-save.
- **Phase B — completion:** **Place on face**, **Scale** + live Size↔Scale linkage + uniform
  toggle, world/local rotate toggle, snap toggle (15°/90°), Reset-to-original, full numeric panel
  polish.

## 9. Provenance (OrcaSlicer references)

Concepts extracted from a sparse clone of `OrcaSlicer/OrcaSlicer@main`:

- Transform decomposition + `T·R·S` order, quaternion-vs-Euler caveat — `src/libslic3r/Geometry.cpp`,
  `Geometry.hpp`.
- `ensure_on_bed` / `min_z` drop-to-bed, instance vs volume transforms — `src/libslic3r/Model.cpp`.
- Place-on-face (`setFromTwoVectors(normal, UnitZ)`, normal matrix, planar clustering we skip) —
  `src/slic3r/GUI/Gizmos/GLGizmoFlatten.cpp`.
- Snapping (rotate 45/5° radial → we use 15°/90° Shift; move 1 mm; scale 5%), world/local space,
  Size↔Scale linkage, reset/drop affordances — `GLGizmoMove/Rotate/Scale.cpp`,
  `GizmoObjectManipulation.cpp`.
- Bed `GROUND_Z` offset, corner-vs-center origin (we use center for a viewer), grid/axes styling —
  `src/slic3r/GUI/3DBed.cpp`.
