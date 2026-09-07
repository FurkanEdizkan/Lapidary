import type {
  BatchId,
  BatchStatus,
  BlobHash,
  ChunkAccepted,
  FolderId,
  FolderNode,
  FolderPatch,
  InstanceStorageView,
  LibraryId,
  PartImage,
  StoredImage,
  LibrarySettings,
  LibraryStorage,
  MovePart,
  NewFolder,
  PartDetail,
  PartId,
  PartsPage,
  PurgeResult,
  RevisionId,
  ScanAccepted,
  UploadFile,
  UploadManifest,
  UploadPlan,
} from './types'
import { strings } from './strings'

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
 * trip. Keyset paged on `after`: the grid holds pages in a `useInfiniteQuery` and asks
 * for the next one from the last id it has, so no page can be skipped or repeated by a
 * part arriving while somebody is scrolling — which an offset would allow.
 *
 * `folderId` filters the page to one category **and everything under it** — the route's
 * filter is subtree-inclusive, so selecting `Terrain` shows what is in `Terrain/Rocks`
 * too. Omitted entirely when nothing is selected rather than sent empty: an absent
 * parameter is what the route reads as "the whole library", and `?folderId=` is a
 * different request nothing promises to answer the same way.
 */
export async function fetchParts(
  library: LibraryId,
  after?: PartId,
  state?: 'removed',
  folderId?: FolderId | null,
): Promise<PartsPage> {
  // Keyset, not offset: `after` is the previous page's last id, and the server orders by
  // id descending. Omitted entirely rather than sent empty — the route reads its absence
  // as "from the top", and `after=` would be a parse error. `folderId` follows the same
  // rule for the reason above it: an absent parameter is what the route reads as "the
  // whole library".
  const query = new URLSearchParams()
  if (after !== undefined) query.set('after', after)
  if (state !== undefined) query.set('state', state)
  if (typeof folderId === 'string' && folderId.length > 0) query.set('folderId', folderId)
  const suffix = query.size === 0 ? '' : `?${query}`
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/parts${suffix}`)
  if (!response.ok) {
    throw new Error(`parts returned ${response.status}`)
  }
  return (await response.json()) as PartsPage
}

/**
 * `DELETE /api/parts/{id}` — remove a part from the library.
 *
 * Not a deletion, and the whole client is built so nobody has to take that on trust: the
 * part reappears intact from {@link restorePart}, its bytes never moved, and the only
 * thing that changed is a column. See `strings.removal` for the wording rules this pairs
 * with.
 */
export async function removePart(part: PartId): Promise<void> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}`, { method: 'DELETE' })
  if (!response.ok) {
    throw new Error(`remove returned ${response.status}`)
  }
}

/** `POST /api/parts/{id}/restore` — undo of {@link removePart}, indefinitely available. */
export async function restorePart(part: PartId): Promise<void> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/restore`, {
    method: 'POST',
  })
  if (!response.ok) {
    throw new Error(`restore returned ${response.status}`)
  }
}

/**
 * `POST /api/parts/{id}/purge` — the permanent one, and only for a part already removed.
 *
 * The route answers 409 for a live part rather than purging it, so a client cannot make
 * this the first click even by accident. The returned counts are of blobs entering
 * quarantine, never of bytes freed: nothing is freed today.
 */
export async function purgePart(part: PartId): Promise<PurgeResult> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/purge`, {
    method: 'POST',
  })
  if (!response.ok) {
    throw new Error(`purge returned ${response.status}`)
  }
  return (await response.json()) as PurgeResult
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

/**
 * How `folders.rs` refuses a create or a rename, plus the shape of a body without a reason.
 *
 * Separate from `MoveRefusalReason` because these are different routes refusing different
 * things, and folding them into one union would offer every caller four reasons it can
 * never see. `'unknown'` covers an old server and a value this client does not recognise,
 * for the same reason it does there.
 *
 * `renamedAfterMove` and `wouldCycle` are deliberately absent: both need a `parentId`, and
 * neither function below ever sends one. If a client here ever grows a reparent, they come
 * back with it — and `renamedAfterMove` needs the test `folders.rs:283` never got.
 */
export type FolderWriteRefusal = 'nameTaken' | 'slugTaken' | 'emptyName' | 'gone' | 'unknown'

const KNOWN_FOLDER_REFUSALS: readonly string[] = ['nameTaken', 'slugTaken', 'emptyName']

/**
 * `noSuchLibrary` and `noSuchFolder` both mean "the thing you named is not there any more",
 * and a user can do exactly one thing about either: reload. They arrive as `gone` rather
 * than as two reasons that would need two sentences saying the same thing.
 */
async function folderRefusal(response: Response): Promise<FolderWriteRefusal> {
  const reason = await refusalReason(response)
  if (reason === 'noSuchLibrary' || reason === 'noSuchFolder') return 'gone'
  return reason !== undefined && KNOWN_FOLDER_REFUSALS.includes(reason)
    ? (reason as FolderWriteRefusal)
    : 'unknown'
}

/** A write that the route answered rather than failed. Both functions below return it. */
export type FolderWritten = { kind: 'written' } | { kind: 'refused'; reason: FolderWriteRefusal }

/**
 * `POST /api/libraries/{id}/folders` — create one category.
 *
 * The slug is not sent and cannot be: `folders.rs:143` derives it with `slugify`, which is
 * the one place that decides what a filesystem may hold, and a client that could name the
 * directory could name one outside the store.
 *
 * `409` and `400` are answers, not failures — a name a sibling already holds is something
 * the user fixes by typing a different one, and throwing would put "check that the api
 * service is running" in front of a typo.
 */
export async function createFolder(
  library: LibraryId,
  parentId: FolderId | null,
  name: string,
): Promise<FolderWritten> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/folders`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ parentId, name } satisfies NewFolder),
  })
  if (response.status === 409 || response.status === 400 || response.status === 404) {
    return { kind: 'refused', reason: await folderRefusal(response) }
  }
  if (!response.ok) {
    throw new Error(`folder create returned ${response.status}`)
  }
  return { kind: 'written' }
}

/**
 * `PATCH /api/folders/{id}` — change a category's name, and nothing else.
 *
 * **The body is `{ name }` and must stay that way.** `FolderPatch.parentId` is optional and
 * nullable, and the two are different requests: omitted means "do not move it", `null`
 * means "move it to the library root". A rename built by spreading an object that happens
 * to carry `parentId: null` would move every renamed category to the root — silently, and
 * on every rename. Sending one field also keeps this away from `409 renamedAfterMove`
 * (`folders.rs:283`), the both-fields branch no client has ever exercised and no test
 * covers.
 *
 * **The directory on disk does not follow the name.** `folder.slug` is the category's
 * address, allocated once at creation; `folder.name` is its label (`DATA.md` §1.1). The
 * dialog that calls this has to say so, because the store is meant to be opened in a file
 * manager and a person fixing a spelling will otherwise go looking for the fixed one.
 */
export async function renameFolder(folder: FolderId, name: string): Promise<FolderWritten> {
  const response = await fetch(`/api/folders/${encodeURIComponent(folder)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name } satisfies FolderPatch),
  })
  if (response.status === 409 || response.status === 400 || response.status === 404) {
    return { kind: 'refused', reason: await folderRefusal(response) }
  }
  if (!response.ok) {
    throw new Error(`folder rename returned ${response.status}`)
  }
  return { kind: 'written' }
}

/**
 * `GET /api/storage` — what the whole store holds, and where it is on the host.
 *
 * Instance-wide, so it takes no library: two of its figures belong to none, and its
 * derivative total is deliberately not the per-library ones added up.
 *
 * `onDisk` asks the server to walk the storage root for the number `du` would give. Off by
 * default because it costs a stat per file, and the four tracked figures are free.
 */
export async function fetchInstanceStorage(onDisk = false): Promise<InstanceStorageView> {
  const response = await fetch(`/api/storage${onDisk ? '?onDisk=true' : ''}`)
  if (!response.ok) {
    throw new Error(`instance storage returned ${response.status}`)
  }
  return (await response.json()) as InstanceStorageView
}

/** `GET /api/parts/{id}/images` — the gallery, in order. */
export async function fetchPartImages(part: PartId): Promise<PartImage[]> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/images`)
  if (!response.ok) {
    throw new Error(`part images returned ${response.status}`)
  }
  return (await response.json()) as PartImage[]
}

/**
 * `POST /api/parts/{id}/images` — attach a picture.
 *
 * The `File` is the whole body, sent as-is. No `FormData`: the route takes one field and
 * reads the file's own header rather than any type we could declare, so a multipart
 * envelope would be a second format for it to parse and nothing gained.
 *
 * The refusals — too large, not an image, too small — are answers rather than failures, and
 * come back with the server's own sentence. That prose is written for the person who picked
 * the file and names the limit it broke, which a generic "upload failed" could not.
 */
export async function uploadPartImage(
  part: PartId,
  file: File,
): Promise<{ kind: 'stored'; stored: StoredImage } | { kind: 'refused'; message: string }> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/images`, {
    method: 'POST',
    body: file,
  })
  if (response.ok) {
    // The size it was stored at, which is how the caller can say so: an image over the
    // server's bound is scaled down on the way in, and that is not a thing to do to
    // somebody's photograph without telling them.
    return { kind: 'stored', stored: (await response.json()) as StoredImage }
  }
  // 413, 415 and 422 are the three the route uses for "your file, not our fault".
  if ([400, 413, 415, 422].includes(response.status)) {
    const body: unknown = await response.json().catch(() => null)
    const message =
      body !== null && typeof body === 'object'
        ? (body as { message?: unknown }).message
        : undefined
    return {
      kind: 'refused',
      message: typeof message === 'string' ? message : strings.images.refusedWithoutReason,
    }
  }
  throw new Error(`image upload returned ${response.status}`)
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

/**
 * `GET /api/libraries/{library}/jobs/{batch}/events` — the same `BatchStatus`, streamed.
 *
 * The URL only; opening the `EventSource` belongs to the component that has to close it
 * again. Exported so the one place that builds this path is the same file every other
 * route path is built in.
 */
export function batchEventsUrl(library: LibraryId, batch: BatchId): string {
  return `/api/libraries/${encodeURIComponent(library)}/jobs/${encodeURIComponent(batch)}/events`
}
