import type {
  BatchId,
  BatchStatus,
  LibraryId,
  LibrarySettings,
  PartId,
  PartsPage,
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
 * `PATCH /api/libraries/{id}` — whether ingest renders a preview for this library.
 *
 * The response echoes the setting that landed, so the toggle reflects what the server
 * now holds rather than what the click assumed. There is no `GET` counterpart: nothing
 * in the API reads a library's settings back, so the toggle's *initial* position is the
 * documented default (design §3.2: on), not a fact read from the server. A library
 * already switched off therefore shows on until someone changes it. Closing that needs a
 * read route, which is a backend change and not this task's.
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
