import type {
  BatchId,
  BatchStatus,
  LibraryId,
  LibrarySettings,
  LibraryStorage,
  PartId,
  PartsPage,
  RevisionId,
  ScanAccepted,
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
export async function fetchParts(library: LibraryId): Promise<PartsPage> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/parts`)
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
 * A scan is started against the worker (`POST /api/libraries/{id}/scan` on port 8081),
 * not from this page: the scan route is mounted under the worker role only, and the web
 * proxy deliberately forwards `/api/*` to the api service. So the batch id arrives here
 * in the URL — `/?batch=<id>` — rather than from a mutation this page issued. A scan the
 * browser can start belongs with the upload path, which is a later slice.
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

/** Both enqueue routes answer alike, so they read the answer alike. */
async function accepted(response: Response): Promise<ScanAccepted> {
  if (!response.ok) {
    throw new Error(`thumbnail render returned ${response.status}`)
  }
  return (await response.json()) as ScanAccepted
}
