import type { Sort } from './preferences'
import type {
  BatchId,
  BatchStatus,
  BlobHash,
  ChunkAccepted,
  Facets,
  FailurePage,
  FetchImageRequest,
  FolderId,
  FolderNode,
  FolderPatch,
  InstanceStorageView,
  RenderCacheFreedView,
  BundlePlan,
  JobId,
  LibraryId,
  MaterialDensity,
  LibraryStorage,
  LibrarySummary,
  MovePart,
  NewFolder,
  NewSavedFilter,
  NewLibrary,
  NewSource,
  PartDetail,
  PartId,
  PartImage,
  PartRevision,
  PartImageId,
  PartSource,
  PartsPage,
  RetryAccepted,
  RevisionId,
  ExportReady,
  SavedFilter,
  SavedFilterId,
  MoveDirection,
  MoveSavedFilter,
  RenameSavedFilter,
  CustomField,
  CustomFieldChange,
  NewCustomField,
  SetDensity,
  SetFieldValue,
  ScanAccepted,
  SetFraming,
  SetMaterials,
  SetTags,
  StoredImage,
  UploadFile,
  UploadManifest,
  UploadPlan,
  CheckSiteKey,
  Profile,
  ImportLink,
  SetSiteKey,
  SiteCheck,
  SiteSettings,
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
  folderId?: FolderId | null,
  q?: string,
  limit?: number,
  format?: string,
  sort?: Sort,
  material?: string,
  tag?: string,
  field?: string,
  fieldValue?: string,
  fieldMin?: string,
  fieldMax?: string,
): Promise<PartsPage> {
  // Keyset, not offset: `after` is the previous page's last id, and the server orders by
  // id descending. Omitted entirely rather than sent empty — the route reads its absence
  // as "from the top", and `after=` would be a parse error. `folderId` follows the same
  // rule for the reason above it: an absent parameter is what the route reads as "the
  // whole library".
  const query = new URLSearchParams()
  if (after !== undefined) query.set('after', after)
  if (typeof folderId === 'string' && folderId.length > 0) query.set('folderId', folderId)
  // Same rule again: omitted rather than sent empty. The route reads `q=` as no filter, so
  // an empty one would work — but a URL carrying a parameter that means nothing is a URL
  // that reads as a search nobody ran.
  if (typeof q === 'string' && q.length > 0) query.set('q', q)
  // Always sent when the caller has one, including the value that happens to match the
  // route's own default. Omitting it there would mean this client knowing what the server's
  // default is — a second copy of a number, in a different language, that goes wrong
  // silently the day somebody changes one of them. The grid has a page size; it says so.
  if (typeof limit === 'number') query.set('limit', String(limit))
  if (typeof format === 'string' && format.length > 0) query.set('format', format)
  // Omitted for newest, the way an absent category is the whole library: newest is the grid
  // with no order asked for, so every URL the grid sent before sorting existed keeps its shape.
  if (sort !== undefined && sort !== 'newest') query.set('sort', sort)
  if (typeof material === 'string' && material.length > 0) query.set('material', material)
  if (typeof tag === 'string' && tag.length > 0) query.set('tag', tag)
  setField(query, field, fieldValue, fieldMin, fieldMax)
  const suffix = query.size === 0 ? '' : `?${query}`
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/parts${suffix}`)
  if (!response.ok) {
    throw new RefusedError(`parts returned ${response.status}`, await refusalReason(response))
  }
  return (await response.json()) as PartsPage
}

/**
 * `GET /api/libraries/{id}/facets` — the formats among the parts the grid shows for the same
 * category and query, with counts. The grid's chosen format is deliberately not sent: counts
 * that obeyed it would show every other format as zero.
 */
export async function fetchFacets(
  library: LibraryId,
  folderId?: FolderId | null,
  q?: string,
  format?: string,
  material?: string,
  tag?: string,
  field?: string,
  fieldValue?: string,
  fieldMin?: string,
  fieldMax?: string,
): Promise<Facets> {
  const query = new URLSearchParams()
  if (typeof folderId === 'string' && folderId.length > 0) query.set('folderId', folderId)
  if (typeof q === 'string' && q.length > 0) query.set('q', q)
  // Both choices, so each list can be narrowed by the other's. The server keeps each list from
  // obeying its own, which would show every other value as zero.
  if (typeof format === 'string' && format.length > 0) query.set('format', format)
  if (typeof material === 'string' && material.length > 0) query.set('material', material)
  if (typeof tag === 'string' && tag.length > 0) query.set('tag', tag)
  setField(query, field, fieldValue, fieldMin, fieldMax)
  const suffix = query.size === 0 ? '' : `?${query}`
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/facets${suffix}`)
  if (!response.ok) {
    throw new RefusedError(`facets returned ${response.status}`, await refusalReason(response))
  }
  return (await response.json()) as Facets
}

/**
 * A custom field filter rides with its value or its range, or not at all: the route reads a key alone, or
 * a value or a bound alone, as no filter, and a URL carrying half of one reads as a filter nobody set.
 */
function setField(
  query: URLSearchParams,
  field?: string,
  fieldValue?: string,
  fieldMin?: string,
  fieldMax?: string,
): void {
  const given = Object.entries({ fieldValue, fieldMin, fieldMax }).filter(
    (entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1].length > 0,
  )
  if (typeof field !== 'string' || field.length === 0 || given.length === 0) return
  query.set('field', field)
  for (const [name, value] of given) query.set(name, value)
}

/** `GET /api/libraries/{id}/fields` — the library's custom fields, oldest first. */
export async function fetchFields(library: LibraryId): Promise<CustomField[]> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/fields`)
  if (!response.ok) {
    throw new Error(`fields returned ${response.status}`)
  }
  return (await response.json()) as CustomField[]
}

/** A field write the route answered rather than failed: saved, or refused in its own words. */
export type FieldWritten = { kind: 'saved' } | { kind: 'refused'; message: string; reason?: string }

async function fieldWritten(
  response: Response,
  fallback: string = strings.fields.refusedWithoutReason,
): Promise<FieldWritten> {
  if (response.ok) return { kind: 'saved' }
  if (response.status === 400 || response.status === 404 || response.status === 409) {
    const answer: unknown = await response.json().catch(() => null)
    const { message, reason } =
      answer !== null && typeof answer === 'object'
        ? (answer as { message?: unknown; reason?: unknown })
        : ({} as { message?: unknown; reason?: unknown })
    return {
      kind: 'refused',
      message: typeof message === 'string' ? message : fallback,
      ...(typeof reason === 'string' ? { reason } : {}),
    }
  }
  throw new Error(`write returned ${response.status}`)
}

/** `POST /api/libraries/{id}/fields` — define a field. */
export async function createField(library: LibraryId, field: NewCustomField): Promise<FieldWritten> {
  return fieldWritten(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/fields`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(field),
    }),
  )
}

/** `PATCH /api/libraries/{library}/fields/{key}` — relabel a field, change its options or filter. */
export async function updateField(
  library: LibraryId,
  key: string,
  change: CustomFieldChange,
): Promise<FieldWritten> {
  return fieldWritten(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/fields/${encodeURIComponent(key)}`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(change),
    }),
  )
}

/** `DELETE /api/libraries/{library}/fields/{key}` — the definition only; every part keeps its value. */
export async function removeField(library: LibraryId, key: string): Promise<FieldWritten> {
  return fieldWritten(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/fields/${encodeURIComponent(key)}`, {
      method: 'DELETE',
    }),
  )
}

/** `GET /api/libraries/{id}/densities` — a library's density per material, in kg/m³. */
export async function fetchDensities(library: LibraryId): Promise<MaterialDensity[]> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/densities`)
  if (!response.ok) {
    throw new Error(`densities returned ${response.status}`)
  }
  return (await response.json()) as MaterialDensity[]
}

/** `PUT /api/libraries/{library}/densities/{material}` — a material's density in kg/m³, replacing its old one. */
export async function setDensity(library: LibraryId, material: string, densityKgM3: number): Promise<FieldWritten> {
  const body: SetDensity = { densityKgM3 }
  return fieldWritten(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/densities/${encodeURIComponent(material)}`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    }),
    strings.densities.refusedWithoutReason,
  )
}

/** `DELETE /api/libraries/{library}/densities/{material}` — a material's density; its parts then have no mass. */
export async function removeDensity(library: LibraryId, material: string): Promise<FieldWritten> {
  return fieldWritten(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/densities/${encodeURIComponent(material)}`, {
      method: 'DELETE',
    }),
    strings.densities.refusedWithoutReason,
  )
}

/** `PUT /api/parts/{id}/fields/{key}` — one part's value, or `null` to clear it. */
export async function setFieldValue(
  part: PartId,
  key: string,
  value: string | number | null,
): Promise<FieldWritten> {
  const body: SetFieldValue = { value }
  return fieldWritten(
    await fetch(`/api/parts/${encodeURIComponent(part)}/fields/${encodeURIComponent(key)}`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    }),
  )
}

/**
 * `DELETE /api/parts/{id}` — delete a part from the library, for good.
 *
 * Its rows, pictures and tags go; the model file on disk is never touched. There is no
 * restore (owner, 2026-10-17), so every caller puts a confirmation in front of this that
 * says both halves — see `strings.removal`.
 */
export async function removePart(part: PartId): Promise<void> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}`, { method: 'DELETE' })
  if (!response.ok) {
    throw new Error(`delete returned ${response.status}`)
  }
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
 * `GET /api/parts/{id}/file` — the part's current file, one link that keeps working when a new
 * revision is recorded. Absolute, because it is for pasting somewhere that is not this page.
 */
export function fileLink(part: PartId, origin: string = window.location.origin): string {
  return `${origin}/api/parts/${encodeURIComponent(part)}/file`
}

/** The 3MF Lapidary wrote from a revision's mesh, for a slicer, named `*.lapidary.3mf`: once `requestExport` says it exists. */
export function exportUrl(revision: RevisionId): string {
  return `/api/revisions/${encodeURIComponent(revision)}/download?variant=3mf`
}

/** `GET /api/profile` — this install's display name (M2). */
export async function fetchProfile(): Promise<Profile> {
  const response = await fetch('/api/profile')
  if (!response.ok) throw new Error(`profile returned ${response.status}`)
  return (await response.json()) as Profile
}

/** `PUT /api/profile` — a blank name clears it. */
export async function saveProfile(displayName: string): Promise<Profile> {
  const response = await fetch('/api/profile', {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ displayName: displayName.trim() === '' ? null : displayName } satisfies Profile),
  })
  if (!response.ok) throw new Error(`saving the profile returned ${response.status}`)
  return (await response.json()) as Profile
}

/**
 * `GET /api/settings/sites` — each model site, whether a key is saved for it (by its last four
 * characters, never the key) and what its last check said.
 */
export async function fetchSiteSettings(): Promise<SiteSettings> {
  const response = await fetch('/api/settings/sites')
  if (!response.ok) throw new Error(`site settings returned ${response.status}`)
  return (await response.json()) as SiteSettings
}

/** `PUT /api/settings/sites/{site}` — save a key, replacing the one there. */
export async function saveSiteKey(site: string, key: string): Promise<void> {
  const response = await fetch(`/api/settings/sites/${encodeURIComponent(site)}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ key } satisfies SetSiteKey),
  })
  if (!response.ok) throw new Error(`saving a site key returned ${response.status}`)
}

/** `DELETE /api/settings/sites/{site}` — forget the key. */
export async function clearSiteKey(site: string): Promise<void> {
  const response = await fetch(`/api/settings/sites/${encodeURIComponent(site)}`, { method: 'DELETE' })
  if (!response.ok) throw new Error(`removing a site key returned ${response.status}`)
}

/**
 * `POST /api/settings/sites/{site}/check` — whether a key works: the one typed, unsaved, or the
 * saved one when none is given. "It does not work" is an answer, not a failure.
 */
export async function checkSiteKey(site: string, key?: string): Promise<SiteCheck> {
  const response = await fetch(`/api/settings/sites/${encodeURIComponent(site)}/check`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify((key === undefined ? {} : { key }) satisfies CheckSiteKey),
  })
  if (response.ok || response.status === 403) {
    const body: unknown = await response.json()
    if (response.ok) return body as SiteCheck
    const message = (body as { message?: unknown }).message
    return { ok: false, account: null, message: typeof message === 'string' ? message : strings.settings.checkFailed }
  }
  throw new Error(`checking a site key returned ${response.status}`)
}

/**
 * `POST /api/libraries/{id}/imports/link` — queue a model's import from its page. A refusal comes
 * back with the server's sentence: the site is not supported, the key is missing, importing is off.
 */
export async function importLink(
  library: LibraryId,
  url: string,
): Promise<{ kind: 'queued'; accepted: ScanAccepted } | { kind: 'refused'; message: string }> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/imports/link`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ url } satisfies ImportLink),
  })
  if (response.ok) return { kind: 'queued', accepted: (await response.json()) as ScanAccepted }
  if ([400, 403, 404, 422].includes(response.status)) {
    const body: unknown = await response.json().catch(() => null)
    const message = body !== null && typeof body === 'object' ? (body as { message?: unknown }).message : undefined
    return { kind: 'refused', message: typeof message === 'string' ? message : strings.importLink.failed }
  }
  throw new Error(`importing a link returned ${response.status}`)
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
 * `GET …/jobs/{batch}/failed?after=` — the failures past the hundred `BatchStatus` carries, a
 * page at a time. `after` is the last job already on screen.
 */
export async function fetchFailures(
  library: LibraryId,
  batch: BatchId,
  after: JobId,
): Promise<FailurePage> {
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/jobs/${encodeURIComponent(batch)}/failed?after=${encodeURIComponent(after)}`,
  )
  if (!response.ok) {
    throw new Error(`failure list returned ${response.status}`)
  }
  return (await response.json()) as FailurePage
}

/**
 * `POST …/jobs/{batch}/retry` — the batch's failed files back in the queue, or only `job`.
 * The batch reopens on the server, so reading its status again is how the page finds out.
 */
export async function retryFailed(
  library: LibraryId,
  batch: BatchId,
  job?: JobId,
): Promise<RetryAccepted> {
  const suffix = job === undefined ? '' : `?job=${encodeURIComponent(job)}`
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/jobs/${encodeURIComponent(batch)}/retry${suffix}`,
    { method: 'POST' },
  )
  if (!response.ok) {
    throw new Error(`retry returned ${response.status}`)
  }
  return (await response.json()) as RetryAccepted
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

/** `GET /api/libraries/{id}/filters`: the library's saved filters, by name. */
export async function fetchSavedFilters(library: LibraryId): Promise<SavedFilter[]> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/filters`)
  if (!response.ok) {
    throw new Error(`saved filters returned ${response.status}`)
  }
  return (await response.json()) as SavedFilter[]
}

/**
 * `POST /api/libraries/{id}/filters`: keep the grid's filters under a name. A refusal (a name
 * already taken, too long, or nothing to keep) comes back with the server's own sentence, to show
 * as it is.
 */
export async function saveFilter(
  library: LibraryId,
  filter: NewSavedFilter,
): Promise<{ kind: 'saved'; filter: SavedFilter } | { kind: 'refused'; message: string }> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/filters`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(filter),
  })
  if (response.ok) return { kind: 'saved', filter: (await response.json()) as SavedFilter }
  if (response.status === 400 || response.status === 404 || response.status === 409) {
    const answer: unknown = await response.json().catch(() => null)
    const message =
      answer !== null && typeof answer === 'object'
        ? (answer as { message?: unknown }).message
        : undefined
    return {
      kind: 'refused',
      message: typeof message === 'string' ? message : strings.savedFilters.refusedWithoutReason,
    }
  }
  throw new Error(`saving a filter returned ${response.status}`)
}

/** `DELETE /api/libraries/{library}/filters/{filter}`. One already gone counts as removed. */
export async function removeSavedFilter(library: LibraryId, filter: SavedFilterId): Promise<void> {
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/filters/${encodeURIComponent(filter)}`,
    { method: 'DELETE' },
  )
  if (!response.ok && response.status !== 404) {
    throw new Error(`removing a saved filter returned ${response.status}`)
  }
}

/**
 * `PATCH /api/libraries/{library}/filters/{filter}`: a new name for a saved filter. A refusal (a name
 * already taken, empty or too long) comes back with the server's own sentence, to show as it is.
 */
export async function renameSavedFilter(
  library: LibraryId,
  filter: SavedFilterId,
  name: string,
): Promise<{ kind: 'renamed' } | { kind: 'refused'; message: string }> {
  const body: RenameSavedFilter = { name }
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/filters/${encodeURIComponent(filter)}`,
    { method: 'PATCH', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) },
  )
  if (response.ok) return { kind: 'renamed' }
  if (response.status === 400 || response.status === 404 || response.status === 409) {
    const answer: unknown = await response.json().catch(() => null)
    const message =
      answer !== null && typeof answer === 'object'
        ? (answer as { message?: unknown }).message
        : undefined
    return {
      kind: 'refused',
      message: typeof message === 'string' ? message : strings.savedFilters.refusedWithoutReason,
    }
  }
  throw new Error(`renaming a saved filter returned ${response.status}`)
}

/** `POST /api/libraries/{library}/filters/{filter}/move`: one place up or down the list. */
export async function moveSavedFilter(
  library: LibraryId,
  filter: SavedFilterId,
  direction: MoveDirection,
): Promise<void> {
  const body: MoveSavedFilter = { direction }
  const response = await fetch(
    `/api/libraries/${encodeURIComponent(library)}/filters/${encodeURIComponent(filter)}/move`,
    { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) },
  )
  if (!response.ok) {
    throw new Error(`moving a saved filter returned ${response.status}`)
  }
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
  | 'crossLibrary'
  | 'duplicateName'
  | 'noSuchFolder'
  | 'unknown'

const KNOWN_MOVE_REFUSAL_REASONS: readonly string[] = [
  'crossLibrary',
  'duplicateName',
  'noSuchFolder',
]

/** A read the server answered with an error, and the `reason` its body named, if any. */
export class RefusedError extends Error {
  readonly reason: string | undefined
  constructor(message: string, reason: string | undefined) {
    super(message)
    this.reason = reason
  }
}

/**
 * The `reason` a refusal names itself with, or `undefined` for a body that carries none
 * and for one that is not JSON at all. Shared by the move's `409` and the delete's `404`:
 * both come off `folders.rs`'s one `refused` helper, and a client that reads the field on
 * one route and discards it on the other reports "already deleted" as a server outage.
 */
export async function refusalReason(response: Response): Promise<string | undefined> {
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
 * of throwing on it — but the status alone does not say which of three refusals it is, so
 * the body's `reason` rides along. Only `duplicateName` is the collision slice 6a decided
 * two models called `bracket` can both be true; the client shows that warning once and
 * re-sends the same target with `acknowledgeDuplicate: true`. The other two —
 * `crossLibrary`, `noSuchFolder` — are dead ends for this attempt, and acknowledging would
 * not change the answer. A move is a database change since M4: nothing on disk can refuse it.
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
 * What a delete actually did, as the route counts it. `foldersDeleted` counts the category
 * itself along with its descendants, so a leaf category with no models answers
 * `{ foldersDeleted: 1, partsDeleted: 0 }`. Subtracting the one is the caller's job, and the
 * one place that compares these against what the confirmation warned about says so where it
 * does it.
 *
 * `null` where a figure did not arrive, never 0. A body this client could not read is not
 * evidence that nothing was deleted, and the caller compares these against the counts its
 * confirmation showed — a missing figure read as 0 would report every successful delete as
 * having done something other than what it warned about.
 */
export type FolderDeleted = {
  kind: 'deleted'
  foldersDeleted: number | null
  partsDeleted: number | null
}

/**
 * `DELETE /api/folders/{id}` — delete a category and everything under it, for good.
 *
 * Cascading: the category, its subcategories and the models in any of them leave the
 * library. No model file leaves the disk, and evicting the derivative cache is a different
 * thing again. The confirmation this route sits behind is where that is spelled out.
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
  const counted = (field: 'foldersDeleted' | 'partsDeleted'): number | null => {
    if (body === null || typeof body !== 'object') return null
    const value = (body as Record<string, unknown>)[field]
    return typeof value === 'number' ? value : null
  }
  return {
    kind: 'deleted',
    foldersDeleted: counted('foldersDeleted'),
    partsDeleted: counted('partsDeleted'),
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
 * `cannotMove` is absent: the route refuses a `parentId`, and `FolderPatch` has no such field
 * to send.
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
 * **The body is `{ name }` and must stay that way.** A category cannot move yet: the route
 * refuses any `parentId`, `null` (the library root) included, with `400 cannotMove`, and renames
 * nothing. `FolderPatch` has no `parentId`, so only a body spread from some other object could
 * carry one.
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

/**
 * `POST /api/libraries/{id}/bundle/plan` — what exporting these parts would put in a bundle, or,
 * thrown, the server's own words for why it will not.
 */
export async function planBundle(
  library: LibraryId,
  parts: PartId[],
): Promise<{ plan: BundlePlan } | { refused: string | null }> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/bundle/plan`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ parts }),
  })
  if (!response.ok) {
    // The server's own words when it gave any, and nothing otherwise: a status code is not a
    // sentence for the grid.
    const body = (await response.json().catch(() => null)) as { message?: string } | null
    return { refused: body?.message ?? null }
  }
  return { plan: (await response.json()) as BundlePlan }
}

/** The hidden frame a bundle downloads into, so a late refusal never replaces the grid. */
const BUNDLE_FRAME = 'lapidary-bundle-download'

/**
 * `POST /api/libraries/{id}/bundle`, as a form the browser posts itself: the bundle streams to
 * disk under the name the server gives it, where a `fetch` would hold the whole archive in memory.
 */
export function downloadBundle(library: LibraryId, parts: PartId[]): void {
  let frame = document.querySelector<HTMLIFrameElement>(`iframe[name="${BUNDLE_FRAME}"]`)
  if (frame === null) {
    frame = document.createElement('iframe')
    frame.name = BUNDLE_FRAME
    frame.hidden = true
    document.body.append(frame)
  }
  const form = document.createElement('form')
  form.method = 'post'
  form.target = BUNDLE_FRAME
  form.action = `/api/libraries/${encodeURIComponent(library)}/bundle`
  const field = document.createElement('input')
  field.type = 'hidden'
  field.name = 'parts'
  field.value = parts.join(',')
  form.append(field)
  document.body.append(form)
  form.submit()
  form.remove()
}

/**
 * `POST /api/storage/render-cache` — "free cache space" (`DATA.md` §1.5): the detailed rungs of
 * parts nobody has opened in 90 days, into quarantine. Never a source file.
 */
export async function freeRenderCache(): Promise<RenderCacheFreedView> {
  const response = await fetch('/api/storage/render-cache', { method: 'POST' })
  if (!response.ok) {
    throw new Error(`freeing the render cache returned ${response.status}`)
  }
  return (await response.json()) as RenderCacheFreedView
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
 * Attach a picture to a part — from a file, or from an address.
 *
 * **One function for both, because the answer is the same shape.** A `File` goes to
 * `POST …/images` as the whole body: no `FormData`, because the route takes one field and
 * reads the file's own header rather than any type we could declare, so a multipart envelope
 * would be a second format for it to parse and nothing gained. A string goes to
 * `POST …/images/from-url` as JSON, and the server fetches it — resolving the address and
 * checking it against every private range itself, which is precisely why this cannot be done
 * from the browser.
 *
 * The refusals — too large, not an image, too small, an address we will not fetch from — are
 * answers rather than failures, and come back with the server's own sentence. That prose is
 * written for the person who picked the file or pasted the link and names what was wrong
 * with it, which a generic "upload failed" could not.
 */
export async function uploadPartImage(
  part: PartId,
  source: Blob | string,
  /** `snapshot` for the 3D viewer's own canvas, recorded as that rather than as a file somebody chose. */
  from?: 'snapshot',
): Promise<{ kind: 'stored'; stored: StoredImage } | { kind: 'refused'; message: string }> {
  const base = `/api/parts/${encodeURIComponent(part)}/images`
  const response =
    typeof source === 'string'
      ? await fetch(`${base}/from-url`, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ url: source } satisfies FetchImageRequest),
        })
      : await fetch(from === undefined ? base : `${base}?from=${from}`, { method: 'POST', body: source })
  if (response.ok) {
    // The size it was stored at, which is how the caller can say so: an image over the
    // server's bound is scaled down on the way in, and that is not a thing to do to
    // somebody's photograph without telling them.
    return { kind: 'stored', stored: (await response.json()) as StoredImage }
  }
  // 413, 415 and 422 are what both routes use for "your file, not our fault"; 502 and 504
  // are the two only the fetch can answer — the address was fine and the other end did not
  // come back. Those carry a sentence too, and throwing past it would replace "that host
  // could not be found" with "something went wrong".
  if ([400, 413, 415, 422, 502, 504].includes(response.status)) {
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

/**
 * `PATCH /api/parts/{id}/images/{imageId}` — how a picture sits in its frame.
 *
 * Three numbers and a word, and **no bytes**: the framing is applied by the browser at
 * display time, so this can be done as often as somebody likes and the picture never loses
 * a generation of quality to it.
 *
 * Both ids go in the path because an image id on its own is a bare handle to a row — the
 * server checks the pair, and a mismatch is a 404.
 */
export async function setImageFraming(
  part: PartId,
  image: PartImageId,
  framing: SetFraming,
): Promise<void> {
  const response = await fetch(
    `/api/parts/${encodeURIComponent(part)}/images/${encodeURIComponent(image)}`,
    {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(framing),
    },
  )
  if (!response.ok) {
    throw new Error(`re-framing returned ${response.status}`)
  }
}

/** `GET /api/parts/{id}/sources` — where a part came from, oldest first. */
export async function fetchPartSources(part: PartId): Promise<PartSource[]> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/sources`)
  if (!response.ok) {
    throw new Error(`part sources returned ${response.status}`)
  }
  return (await response.json()) as PartSource[]
}

/**
 * `POST /api/parts/{id}/sources` — record where a part came from.
 *
 * **The link is stored, not followed.** Nothing here fetches the page: the one route in the
 * application that makes an outbound request is the image fetch, and keeping it the only one
 * is what makes that boundary findable.
 *
 * The refusals — a price with no currency, a form of empty boxes — come back with the
 * server's own sentence, for the same reason the image ones do.
 */
export async function addPartSource(
  part: PartId,
  source: NewSource,
): Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/sources`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(source),
  })
  if (response.ok) return { kind: 'saved' }
  if (response.status === 422) {
    const body: unknown = await response.json().catch(() => null)
    const message =
      body !== null && typeof body === 'object'
        ? (body as { message?: unknown }).message
        : undefined
    return {
      kind: 'refused',
      message: typeof message === 'string' ? message : strings.sources.refusedWithoutReason,
    }
  }
  throw new Error(`recording a source returned ${response.status}`)
}

/**
 * `PUT /api/parts/{id}/tags` — replace a part's tags with `tags`. The server trims them and drops
 * blanks and repeats; a list past its limits, or a part that is gone, comes back refused with the
 * server's own sentence.
 */
export async function setPartTags(
  part: PartId,
  tags: readonly string[],
): Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }> {
  const body: SetTags = { tags: [...tags] }
  return putList(`/api/parts/${encodeURIComponent(part)}/tags`, body, strings.tags.refusedWithoutReason, 'tags')
}

/**
 * `PUT /api/parts/{id}/materials` — replace a part's materials with `materials`, kept over what its file
 * states. An **empty list says the part holds none**, and is kept just as firmly;
 * [`unsetPartMaterials`] is what hands the part back to its file. Refused as tags are.
 */
export async function setPartMaterials(
  part: PartId,
  materials: readonly string[],
): Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }> {
  const body: SetMaterials = { materials: [...materials] }
  return putList(
    `/api/parts/${encodeURIComponent(part)}/materials`,
    body,
    strings.materials.refusedWithoutReason,
    'materials',
  )
}

/**
 * `DELETE /api/parts/{id}/materials` — hand the part back to what its file states, now and at every
 * later read of it. Takes the same shape of answer as the `PUT`, so one caller handles both.
 */
export async function unsetPartMaterials(
  part: PartId,
): Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }> {
  return putList(
    `/api/parts/${encodeURIComponent(part)}/materials`,
    undefined,
    strings.materials.refusedWithoutReason,
    'materials',
    'DELETE',
  )
}

/** One whole list PUT to a part: saved, or refused in the server's own sentence, else `fallback`. */
async function putList(
  path: string,
  body: SetTags | SetMaterials | undefined,
  fallback: string,
  what: string,
  method: 'PUT' | 'DELETE' = 'PUT',
): Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }> {
  const response = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (response.ok) return { kind: 'saved' }
  if (response.status === 400 || response.status === 404) {
    const answer: unknown = await response.json().catch(() => null)
    const message =
      answer !== null && typeof answer === 'object'
        ? (answer as { message?: unknown }).message
        : undefined
    return { kind: 'refused', message: typeof message === 'string' ? message : fallback }
  }
  throw new Error(`saving ${what} returned ${response.status}`)
}

/**
 * `GET /api/libraries` — every library, oldest first.
 *
 * Unpaginated: a deployment has a handful of these, not a page of them, and a switcher that
 * paged would be a control nobody could scan.
 */
export async function fetchLibraries(): Promise<LibrarySummary[]> {
  const response = await fetch('/api/libraries')
  if (!response.ok) {
    throw new Error(`libraries returned ${response.status}`)
  }
  return (await response.json()) as LibrarySummary[]
}

/**
 * `POST /api/libraries` — make one.
 *
 * `409 nameTaken` is an answer rather than a failure: two libraries with one name is a
 * switcher nobody can use, and the fix is typing a different name.
 */
export async function createLibrary(
  body: NewLibrary,
): Promise<{ kind: 'created'; library: LibrarySummary } | { kind: 'refused'; message: string }> {
  const response = await fetch('/api/libraries', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (response.ok) {
    return { kind: 'created', library: (await response.json()) as LibrarySummary }
  }
  if (response.status === 409 || response.status === 400) {
    const parsed: unknown = await response.json().catch(() => null)
    const message =
      parsed !== null && typeof parsed === 'object'
        ? (parsed as { message?: unknown }).message
        : undefined
    return {
      kind: 'refused',
      message: typeof message === 'string' ? message : strings.libraries.refusedWithoutReason,
    }
  }
  throw new Error(`library create returned ${response.status}`)
}

/** Every enqueue route answers alike, so they read the answer alike. */
async function accepted(response: Response): Promise<ScanAccepted> {
  if (!response.ok) {
    throw new Error(`enqueue returned ${response.status}`)
  }
  return (await response.json()) as ScanAccepted
}

/**
 * A `400` from an upload route, carrying the server's own sentence.
 *
 * A distinct type because the caller has to tell the two apart: `upload probe returned 500` is
 * for a log, and "Lapidary is a visual library of meshes and does not read STEP files…" is for
 * the person who just dropped one. The drop UI shows this one and its own generic line for
 * anything else.
 */
export class UploadRefused extends Error {}

/** The refusal's message, if this response is one. */
async function refusal(response: Response): Promise<UploadRefused | undefined> {
  if (response.status !== 400) {
    return undefined
  }
  const parsed: unknown = await response.json().catch(() => null)
  const message =
    parsed !== null && typeof parsed === 'object'
      ? (parsed as { message?: unknown }).message
      : undefined
  return typeof message === 'string' && message !== '' ? new UploadRefused(message) : undefined
}

/**
 * `POST /api/libraries/{id}/uploads/probe` — which of these files this library still needs.
 *
 * Two lists: `have` is dropped on the floor, and `needBytes` is sent and committed. A re-drop
 * of a folder the library already holds moves nothing, which is the whole reason this route
 * exists.
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
    throw (await refusal(response)) ?? new Error(`upload probe returned ${response.status}`)
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
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/uploads/commit`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ files } satisfies UploadManifest),
  })
  // `refusal` reads the body only on a 400, which it then throws, so `accepted` still has one
  // to read on every other answer.
  const refused = await refusal(response)
  if (refused !== undefined) {
    throw refused
  }
  return accepted(response)
}

/**
 * `POST /api/libraries/{id}/imports` — a bundle already sent through the chunked upload, queued
 * for the worker to check whole and unpack into parts (Phase 4 slice 2 spec §7).
 */
export async function startImport(
  library: LibraryId,
  blake3: BlobHash,
  name: string,
): Promise<ScanAccepted> {
  return accepted(
    await fetch(`/api/libraries/${encodeURIComponent(library)}/imports`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ blake3, name }),
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
 * `GET /api/parts/{id}/revisions` — every revision of a part, newest first. Rows and inline
 * thumbnails, so the open path; a deleted part 404s here as its page does.
 */
export async function fetchRevisions(part: PartId): Promise<PartRevision[]> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/revisions`)
  if (!response.ok) {
    throw new Error(`revision history returned ${response.status}`)
  }
  return (await response.json()) as PartRevision[]
}

/**
 * `POST /api/parts/{id}/exports/3mf` — ask for a part's mesh written as a 3MF, for a slicer.
 *
 * The one thing a worker still builds on demand. The server answers with the file's hash when it
 * already exists, and otherwise queues it and answers with the batch to watch; the file downloads
 * from `exportUrl`.
 */
export async function requestExport(
  part: PartId,
): Promise<{ kind: 'ready'; hash: BlobHash } | { kind: 'queued'; queued: ScanAccepted }> {
  return requestBuilt(`/api/parts/${encodeURIComponent(part)}/exports/3mf`)
}

/** A file Lapidary builds when asked: its hash when it exists, else the batch building it. A refusal throws the server's message. */
async function requestBuilt(
  path: string,
): Promise<{ kind: 'ready'; hash: BlobHash } | { kind: 'queued'; queued: ScanAccepted }> {
  const response = await fetch(path, { method: 'POST' })
  if (response.status === 200) {
    return { kind: 'ready', hash: ((await response.json()) as ExportReady).hash }
  }
  if (response.status === 202) {
    return { kind: 'queued', queued: (await response.json()) as ScanAccepted }
  }
  const refusal = (await response.json().catch(() => null)) as { message?: string } | null
  throw new Error(refusal?.message ?? `${path} returned ${response.status}`)
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
