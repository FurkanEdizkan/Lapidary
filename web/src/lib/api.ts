import type {
  BatchId,
  BatchStatus,
  FolderId,
  FolderNode,
  LibraryId,
  LibrarySettings,
  LibraryStorage,
  MovePart,
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
 *
 * `folderId` filters the page to one category **and everything under it** — the route's
 * filter is subtree-inclusive, so selecting `Terrain` shows what is in `Terrain/Rocks`
 * too. Omitted entirely when nothing is selected rather than sent empty: an absent
 * parameter is what the route reads as "the whole library", and `?folderId=` is a
 * different request nothing promises to answer the same way.
 */
export async function fetchParts(
  library: LibraryId,
  folderId?: FolderId | null,
): Promise<PartsPage> {
  const filter =
    typeof folderId === 'string' && folderId.length > 0
      ? `?folderId=${encodeURIComponent(folderId)}`
      : ''
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/parts${filter}`)
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

/**
 * `GET /api/libraries/{id}/folders` — the category tree, whole, in one request.
 *
 * Not a level at a time. At corpus scale the tree is hundreds of rows, and a lazy tree
 * costs a round trip per expand on the one interaction that has to feel instant (design
 * §10). `FolderNode.partCount` rides along on every node, which is what lets the delete
 * confirmation name the number of models it affects without a second request per folder.
 */
export async function fetchFolders(library: LibraryId): Promise<FolderNode[]> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/folders`)
  if (!response.ok) {
    throw new Error(`folders returned ${response.status}`)
  }
  return (await response.json()) as FolderNode[]
}

/**
 * The four causes `moves.rs`'s `refused` helper puts on a `409` body's `reason` field, plus
 * the shape a body without one takes. Not a `ts-rs` binding — the route hands the reason
 * back as a bare string on an ad hoc JSON object, not a Rust enum on the wire — so this is
 * hand-written the way `MovePart`'s own request shape is, right below.
 *
 * `'unknown'` covers two cases on purpose: an old server (before this field shipped) and a
 * value this client does not recognise. Both have to fall here and nowhere else — folding
 * either into `'duplicateName'` would open the acknowledge-and-retry dialog on a refusal
 * that acknowledging can never fix.
 */
export type MoveRefusalReason =
  | 'migrationPending'
  | 'crossLibrary'
  | 'duplicateName'
  | 'noSuchFolder'
  | 'unknown'

const KNOWN_MOVE_REFUSAL_REASONS: readonly string[] = [
  'migrationPending',
  'crossLibrary',
  'duplicateName',
  'noSuchFolder',
]

/**
 * The `reason` a refusal names itself with, or `undefined` for a body that carries none
 * and for one that is not JSON at all. Shared by the move's `409` and the delete's `404`:
 * both come off `folders.rs`'s one `refused` helper, and a client that reads the field on
 * one route and discards it on the other reports "already deleted" as a server outage.
 */
async function refusalReason(response: Response): Promise<string | undefined> {
  try {
    const body: unknown = await response.json()
    const reason =
      body !== null && typeof body === 'object' ? (body as { reason?: unknown }).reason : undefined
    return typeof reason === 'string' ? reason : undefined
  } catch {
    return undefined
  }
}

async function moveRefusalReason(response: Response): Promise<MoveRefusalReason> {
  const reason = await refusalReason(response)
  // A `409` with an unrecognised reason, or none, or a body that is not JSON at all — no
  // more readable than each other, and none of them a reason to guess `duplicateName`.
  return reason !== undefined && KNOWN_MOVE_REFUSAL_REASONS.includes(reason)
    ? (reason as MoveRefusalReason)
    : 'unknown'
}

/**
 * `PATCH /api/parts/{id}` — file a model under a category, or under none.
 *
 * `409` is an answer rather than a failure, which is why this returns an outcome instead
 * of throwing on it — but the status alone does not say which of four refusals it is, so
 * the body's `reason` rides along. Only `duplicateName` is the collision slice 6a decided
 * two models called `bracket` can both be true; the client shows that warning once and
 * re-sends the same target with `acknowledgeDuplicate: true`. The other three —
 * `migrationPending`, `crossLibrary`, `noSuchFolder` — are dead ends for this attempt, and
 * acknowledging would not change the answer.
 *
 * Both fields are always sent. The route defaults `acknowledgeDuplicate`, but a request
 * that omitted it would be indistinguishable on the wire from one that meant `false`, and
 * the first request in a pair genuinely means `false`.
 *
 * `folderId: null` is a real target — the library root, no category — and is not the same
 * as leaving the field out.
 */
export async function movePart(
  part: PartId,
  folderId: FolderId | null,
  acknowledgeDuplicate: boolean,
): Promise<{ kind: 'moved' } | { kind: 'refused'; reason: MoveRefusalReason }> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ folderId, acknowledgeDuplicate } satisfies MovePart),
  })
  if (response.status === 409) {
    return { kind: 'refused', reason: await moveRefusalReason(response) }
  }
  if (!response.ok) {
    throw new Error(`move returned ${response.status}`)
  }
  return { kind: 'moved' }
}

/**
 * What a delete actually did, as the route counts it. `foldersHidden` counts the category
 * itself along with its descendants — `soft_delete_subtree` reports `rows_affected` over
 * the whole subtree — so a leaf category with no models answers `{ foldersHidden: 1,
 * partsHidden: 0 }`. Subtracting the one is the caller's job, and the one place that
 * compares these against what the confirmation warned about says so where it does it.
 *
 * `null` where a figure did not arrive, never 0. A body this client could not read is not
 * evidence that nothing was hidden, and the caller compares these against the counts its
 * confirmation showed — a missing figure read as 0 would report every successful delete as
 * having done something other than what it warned about.
 */
export type FolderDeleted = {
  kind: 'deleted'
  foldersHidden: number | null
  partsHidden: number | null
}

/**
 * `DELETE /api/folders/{id}` — soft-delete a category and everything under it.
 *
 * Soft, and cascading: the category, its subcategories and the models in any of them are
 * marked deleted. Nothing leaves the disk — `DATA.md` §1.6's purge is a separate,
 * explicit action, and evicting the derivative cache is a third thing again. The
 * confirmation this route sits behind is where that distinction is spelled out for a user.
 *
 * `404 noSuchFolder` is an answer and not a failure, the same way the move's `409` is: the
 * category was already deleted somewhere else, nothing here is broken, and retrying can
 * only ask about the same missing row again. Throwing on it landed the user on "check that
 * the api service is running", which is wrong in both halves.
 *
 * Only a 404 that names itself gets that treatment. A bare 404 — a proxy that lost the
 * route, a server that is not ours — carries no `reason` and still throws, because
 * "already deleted" is a claim about the library that a misrouted request cannot support.
 */
export async function deleteFolder(
  folder: FolderId,
): Promise<FolderDeleted | { kind: 'refused'; reason: 'noSuchFolder' }> {
  const response = await fetch(`/api/folders/${encodeURIComponent(folder)}`, { method: 'DELETE' })
  if (response.status === 404 && (await refusalReason(response)) === 'noSuchFolder') {
    return { kind: 'refused', reason: 'noSuchFolder' }
  }
  if (!response.ok) {
    throw new Error(`folder delete returned ${response.status}`)
  }
  // Read defensively rather than cast: a `200` whose body is empty, null or not JSON is
  // still a delete that happened, and the counts are a bonus the caller can do without.
  const body: unknown = await response.json().catch(() => null)
  const counted = (field: 'foldersHidden' | 'partsHidden'): number | null => {
    if (body === null || typeof body !== 'object') return null
    const value = (body as Record<string, unknown>)[field]
    return typeof value === 'number' ? value : null
  }
  return {
    kind: 'deleted',
    foldersHidden: counted('foldersHidden'),
    partsHidden: counted('partsHidden'),
  }
}

/** Every enqueue route answers alike, so they read the answer alike. */
async function accepted(response: Response): Promise<ScanAccepted> {
  if (!response.ok) {
    throw new Error(`enqueue returned ${response.status}`)
  }
  return (await response.json()) as ScanAccepted
}
