import type {
  BatchId,
  BatchStatus,
  BlobHash,
  ChunkAccepted,
  LibraryId,
  LibrarySettings,
  LibraryStorage,
  PartDetail,
  PartId,
  PartsPage,
  RevisionId,
  ScanAccepted,
  UploadFile,
  UploadManifest,
  UploadPlan,
} from './types'

export interface Health {
  status: string
  database: { major: number; reachable: boolean }
}

export async function fetchHealth(): Promise<Health> {
  const response = await fetch('/api/healthz')
  if (!response.ok) {
    throw new Error(`healthz returned ${response.status}`)
  }
  return (await response.json()) as Health
}

/**
 * The library seeded by migration `0002_parts.sql`. Slice 1 has no library picker and
 * no route parameter to read one from, so the grid addresses the seeded library
 * directly rather than inventing a selection UI the API cannot yet serve.
 */
export const DEFAULT_LIBRARY_ID: LibraryId = '01931b6e-0000-7000-8000-000000000001'

/**
 * `GET /api/libraries/{id}/parts` — the grid's one read. Thumbnails arrive inline as
 * `data:` URLs, so a page of cards costs this single request and no per-card round
 * trip. Keyset paging (`after`, `limit`) is left for the slice that virtualizes the
 * grid; asking for a page and rendering it is the whole of slice 1.
 */
export async function fetchParts(
  library: LibraryId,
  after?: PartId,
): Promise<PartsPage> {
  // Keyset, not offset: `after` is the previous page's last id, and the server orders by
  // id descending. Omitted entirely rather than sent empty — the route reads its absence
  // as "from the top", and `after=` would be a parse error.
  const query = after === undefined ? '' : `?after=${encodeURIComponent(after)}`
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/parts${query}`)
  if (!response.ok) {
    throw new Error(`parts returned ${response.status}`)
  }
  return (await response.json()) as PartsPage
}

/**
 * `GET /api/revisions/{id}/download?variant=original` — the exact bytes that were
 * ingested.
 *
 * A URL, not a fetch. The card renders it as `<a href download>` and the browser is what
 * reads `Content-Disposition`, including the RFC 5987 `filename*` the route works to get
 * right so that a part named `Gövde plakası` keeps its name in the save dialog. Pulling
 * the bytes through `fetch` into a blob URL would discard that header and name every
 * download after the revision id instead.
 *
 * `variant` is spelled out and never defaults. The route answers 400 without it on
 * purpose (`DATA.md` §5.1): a download that quietly returns something other than what
 * was asked for is the failure that section exists to forbid, and a client omitting the
 * parameter is asking for exactly that.
 */
export function downloadUrl(revision: RevisionId): string {
  return `/api/revisions/${encodeURIComponent(revision)}/download?variant=original`
}

/**
 * `GET /api/libraries/{id}/storage` — what this library occupies on disk, by storage
 * class, and the ratio between the two.
 *
 * Its own query rather than a field on the grid's page, for the reason the settings read
 * is its own: the totals cover the whole library while a page covers 50 parts, so
 * summing the cards on screen would report a library of 200 as a quarter of its size.
 *
 * A 404 is a real answer: no library with that id. Deliberately not softened into zeroes
 * here — `0 B` for an id that names nothing is a number a person would believe.
 */
export async function fetchLibraryStorage(library: LibraryId): Promise<LibraryStorage> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/storage`)
  if (!response.ok) {
    throw new Error(`library storage returned ${response.status}`)
  }
  return (await response.json()) as LibraryStorage
}

/**
 * `GET /api/libraries/{library}/jobs/{batch}` — how a scan is going.
 *
 * The batch id comes from `startScan`'s `202`, or from `/?batch=<id>` for a scan started
 * with the worker's own `curl`. Both are the same resource and the same poll.
 *
 * `total` is the number to watch and `queued` is not: a scan answers `queued: 1` — the
 * directory walk — and the walk then enqueues its files into this same batch, so `total`
 * climbs from 1 while the poll is already running.
 *
 * A 404 is a real answer, not only a failure: it is what an id from another library, an
 * id that was never issued, and a scan that queued nothing all look like.
 */
export async function fetchBatchStatus(
  library: LibraryId,
  batch: BatchId,
): Promise<BatchStatus> {
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/jobs/${encodeURIComponent(batch)}`,
  )
  if (!response.ok) {
    throw new Error(`batch status returned ${response.status}`)
  }
  return (await response.json()) as BatchStatus
}

/**
 * `GET /api/libraries/{id}` — what this library's settings actually are.
 *
 * The toggle's starting position, and the reason it is not design §3.2's documented
 * default: a library already switched off used to render as on until somebody changed it,
 * which is a control misreporting the state it controls.
 *
 * A 404 is a real answer: no library with that id. It is deliberately not softened into
 * the default here — the toggle would then be confidently wrong about a library that does
 * not exist, which is the same lie one level down.
 */
export async function fetchLibrarySettings(library: LibraryId): Promise<LibrarySettings> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}`)
  if (!response.ok) {
    throw new Error(`library settings returned ${response.status}`)
  }
  return (await response.json()) as LibrarySettings
}

/**
 * `PATCH /api/libraries/{id}` — whether ingest renders a preview for this library.
 *
 * The response echoes the setting that landed, so the toggle reflects what the server
 * now holds rather than what the click assumed. It answers the very type
 * `fetchLibrarySettings` reads, so the two cannot disagree about the shape.
 *
 * A 404 is a real answer: no library with that id.
 */
export async function setAutoThumbnail(
  library: LibraryId,
  autoThumbnail: boolean,
): Promise<LibrarySettings> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ autoThumbnail } satisfies LibrarySettings),
  })
  if (!response.ok) {
    throw new Error(`library settings returned ${response.status}`)
  }
  return (await response.json()) as LibrarySettings
}

/**
 * `POST /api/libraries/{id}/scan` — walk the directory mounted on the worker and ingest
 * every model in it.
 *
 * On `Role::Api`, which is what makes a scan button possible at all: `deploy/web/Caddyfile`
 * and vite's dev proxy both forward `/api/*` to the api service and nothing to the worker.
 * The api container mounts no ingest directory, so this route enqueues one `scan_directory`
 * job and the worker does the walk — see `crates/lapidary-ingest/src/scan.rs`.
 *
 * Hence `queued: 1` on success, always: one job, not one per file. The files join the same
 * batch as the walk finds them, which is why the poll watches `total`.
 */
export async function startScan(library: LibraryId): Promise<ScanAccepted> {
  return accepted(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/scan`, { method: 'POST' }),
  )
}

/**
 * `POST /api/libraries/{id}/thumbnails` — render every preview this library is missing.
 *
 * Answers `202` with the same `ScanAccepted` a scan answers with, which is why the batch
 * poll above needs no change to watch a sweep drain. `queued: 0` is a success, not an
 * error — every part already has a preview — and such a batch has no status resource, so
 * the caller must not poll it.
 */
export async function renderLibraryThumbnails(library: LibraryId): Promise<ScanAccepted> {
  return accepted(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/thumbnails`, { method: 'POST' }),
  )
}

/** `POST /api/parts/{id}/thumbnail` — render one part's preview. A batch of one. */
export async function renderPartThumbnail(part: PartId): Promise<ScanAccepted> {
  return accepted(
    await fetch(`/api/parts/${encodeURIComponent(part)}/thumbnail`, { method: 'POST' }),
  )
}

/** Every enqueue route answers alike, so they read the answer alike. */
async function accepted(response: Response): Promise<ScanAccepted> {
  if (!response.ok) {
    throw new Error(`enqueue returned ${response.status}`)
  }
  return (await response.json()) as ScanAccepted
}

/**
 * `POST /api/libraries/{id}/uploads/probe` — which of these files this library still
 * needs, and which of those need their bytes.
 *
 * Three lists, and the client acts on all three differently: `have` is dropped on the
 * floor, `needRows` skips the transfer and goes straight into the commit manifest, and
 * only `needBytes` is sent. Uploading everything anyway would still be *correct* — the
 * store is content-addressed and the worker's short-circuit would settle the duplicates
 * — but re-importing a 25 GB corpus would move 25 GB, which is the whole reason this
 * route exists.
 */
export async function probeUpload(
  library: LibraryId,
  files: UploadFile[],
): Promise<UploadPlan> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/uploads/probe`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ files } satisfies UploadManifest),
  })
  if (!response.ok) {
    throw new Error(`upload probe returned ${response.status}`)
  }
  return (await response.json()) as UploadPlan
}

/**
 * `PUT /api/libraries/{id}/uploads/{blake3}?offset=N` — one chunk, appended.
 *
 * The body is a `Blob` from `File.slice`, never an `ArrayBuffer`: `fetch` streams a blob
 * off disk, so a chunk of a 2 GB file never becomes 8 MB of JavaScript heap and the file
 * itself is never read whole. That is the same mistake the download route was fixed for,
 * on this side of the wire.
 *
 * A `409` is not a failure. It is the server saying where it actually is, in the same
 * `received` field a success answers with, and the caller resumes from there.
 */
export async function putChunk(
  library: LibraryId,
  blake3: BlobHash,
  offset: number,
  chunk: Blob,
): Promise<ChunkAccepted> {
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/uploads/${encodeURIComponent(blake3)}?offset=${offset}`,
    { method: 'PUT', body: chunk },
  )
  if (response.ok || response.status === 409) {
    return (await response.json()) as ChunkAccepted
  }
  throw new Error(`upload chunk returned ${response.status}`)
}

/**
 * `POST /api/libraries/{id}/uploads/commit` — verify what was transferred, store it, and
 * queue the meshing.
 *
 * One call for the whole drop, not one per file: the route mints one batch, and a folder
 * of 500 parts committed file by file would give the grid 500 progress bars. Answers the
 * same `ScanAccepted` every other trigger route answers, which is why the existing batch
 * poll watches an upload with no change at all.
 */
export async function commitUpload(
  library: LibraryId,
  files: UploadFile[],
): Promise<ScanAccepted> {
  return accepted(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/uploads/commit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ files } satisfies UploadManifest),
    }),
  )
}

/**
 * `GET /api/parts/{id}` — one part, in full.
 *
 * The page a card links to. Its own request rather than a field on the grid's page: the
 * bounding box, provenance and format a detail page shows would be paid for fifty times
 * per grid page to be displayed once.
 *
 * A 404 is a real answer, and it is deliberately the same one for a deleted part and an id
 * that names nothing — the route does not distinguish them, because doing so would confirm
 * a part exists to someone who cannot see it.
 */
export async function fetchPartDetail(part: PartId): Promise<PartDetail> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}`)
  if (!response.ok) {
    throw new Error(`part detail returned ${response.status}`)
  }
  return (await response.json()) as PartDetail
}

/**
 * `GET /api/blob/{blake3}` — derivative bytes by hash.
 *
 * A URL, not a fetch, exactly as `downloadUrl` is: the detail page renders it as an
 * `<a href download>` and lets the browser do the transfer. Until the L0 rung's hash
 * reached the client this route had no possible caller at all.
 *
 * Holding a hash is not authorization — the route checks that some part in some library
 * still reaches these bytes before serving one.
 */
export function blobUrl(hash: BlobHash): string {
  return `/api/blob/${encodeURIComponent(hash)}`
}
