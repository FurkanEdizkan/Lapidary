import type {
  DuplicateCluster,
  DuplicateClusters,
  Fold,
  FoldPart,
  LibraryId,
  Likeness,
  PartCard,
  PartId,
  PartLinkKind,
  SetLink,
} from './types'

/**
 * Phase 6's likeness routes, and the two pure functions the screens need around them.
 *
 * Its own module rather than more of `lib/api.ts`, for the reason `phase-6.md` makes the
 * links table its own: this is one feature with one vocabulary, and the vocabulary is the
 * part that must not drift. Three words reach a person — *identical*, *near-duplicate*,
 * *similar* — and no number ever does. That is not a styling choice: the comparison is a
 * Hellinger distance on a 35-float descriptor, and 0.038 is a figure nobody outside this
 * repository can calibrate, so showing it would invite a judgement it cannot support. The
 * wire types carry no score at all, which is how the rule is kept rather than remembered.
 */

/**
 * `GET /api/parts/{id}/likeness` — the four lists for one part.
 *
 * `profiled: false` is an answer and not an empty one: it means the worker has not made this
 * part's shape profile yet, so `nearDuplicates` and `similar` are empty because nothing was
 * compared. The section says so instead of showing four empty lists, which would read as
 * "nothing here is alike".
 */
export async function fetchLikeness(part: PartId): Promise<Likeness> {
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/likeness`)
  if (!response.ok) {
    throw new Error(`likeness returned ${response.status}`)
  }
  return (await response.json()) as Likeness
}

/**
 * `GET /api/libraries/{id}/duplicates` — the review queue, computed when read and never
 * stored.
 *
 * `since` narrows it to the groups a batch produced, which is what the finished upload line
 * links to: after adding eleven files, the queue for the whole library is the wrong page to
 * land on.
 */
export async function fetchDuplicates(library: LibraryId, since?: string): Promise<DuplicateClusters> {
  const query = since === undefined ? '' : `?since=${encodeURIComponent(since)}`
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/duplicates${query}`)
  if (!response.ok) {
    throw new Error(`duplicates returned ${response.status}`)
  }
  return (await response.json()) as DuplicateClusters
}

/**
 * `PUT /api/parts/{id}/links/{other}` — "they belong together", or "never propose this pair
 * again". Both take the pair out of the queue for good.
 *
 * `foldedInto` is excluded at the type level rather than checked at runtime: folding removes
 * a part and is {@link foldPart}'s, and the route refuses it here. `PartLinkKind` is the wire
 * union, so a fourth kind added in Rust reaches this signature through `tsc`.
 */
export async function setPartLink(
  part: PartId,
  other: PartId,
  kind: Exclude<PartLinkKind, 'foldedInto'>,
): Promise<void> {
  const body: SetLink = { kind }
  const response = await fetch(
    `/api/parts/${encodeURIComponent(part)}/links/${encodeURIComponent(other)}`,
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    },
  )
  if (!response.ok) {
    throw new Error(`link returned ${response.status}`)
  }
}

/**
 * `POST /api/parts/{id}/fold` — fold `part` into `into`.
 *
 * **The part in the URL is the one that goes.** `FoldPart.into` names the part kept, so
 * folding the near-duplicate on A's page is a POST against the *near-duplicate*, not against
 * A. Swapping the two removes the part whose page the person is standing on, which is why the
 * argument order here reads the same way as the route's and the test asserts both halves.
 *
 * Never called a merge. `CLAUDE.md` reserves "merge" for versioning, which has none; and this
 * is the soft delete, so the folded part keeps everything and Restore brings it back.
 */
export async function foldPart(part: PartId, into: PartId): Promise<void> {
  const body: FoldPart = { into }
  const response = await fetch(`/api/parts/${encodeURIComponent(part)}/fold`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!response.ok) {
    throw new Error(`fold returned ${response.status}`)
  }
}

/**
 * `GET /api/libraries/{id}/folds` — which removed parts were folded, and into what.
 *
 * Read by the Removed list so a folded part says where it went. A part folded into one that
 * was later purged has no row here and shows nothing special: purge takes a link from either
 * side, so it is an ordinary removed part from then on, and `phase-6.md` calls that expected
 * rather than an orphan.
 */
export async function fetchFolds(library: LibraryId): Promise<Fold[]> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/folds`)
  if (!response.ok) {
    throw new Error(`folds returned ${response.status}`)
  }
  return (await response.json()) as Fold[]
}

/** The newest part in a group, as milliseconds. `-Infinity` for a group with no readable date. */
function newestAt(cluster: DuplicateCluster): number {
  return cluster.parts.reduce((newest, part) => Math.max(newest, at(part)), Number.NEGATIVE_INFINITY)
}

/**
 * One card's `createdAt` as milliseconds, or `-Infinity`.
 *
 * `Date.parse`, not a string comparison: the wire is RFC 3339 and a server is free to send
 * `+03:00` where another sends `Z`, which sorts wrongly as text while naming the same instant.
 * `typeof`, because the response is cast rather than validated.
 */
function at(part: PartCard): number {
  if (typeof part.createdAt !== 'string') return Number.NEGATIVE_INFINITY
  const parsed = Date.parse(part.createdAt)
  return Number.isNaN(parsed) ? Number.NEGATIVE_INFINITY : parsed
}

/**
 * The queue's order: the group holding the most recently added part first.
 *
 * Sorted here rather than trusted from the wire. `DuplicateClusters` promises an order
 * *within* a group (largest first) and says nothing about the order of the groups, and
 * "newest first" is what makes the page useful after an upload — so the client sorts, and the
 * test is deterministic either way. `toSorted` would need ES2023; `lib` is ES2022.
 */
export function newestFirst(clusters: readonly DuplicateCluster[]): DuplicateCluster[] {
  return [...clusters].sort((a, b) => newestAt(b) - newestAt(a))
}

/**
 * How many of the parts added since `since` are in a group with something else.
 *
 * Parts, not groups, because that is the sentence a person needs after an upload: "3 of the
 * files you just added look like parts already here". Counting groups answers a different
 * question and would report 1 for a batch of eleven copies of one part.
 *
 * An unparseable `since` counts nothing rather than everything: a line that over-reports
 * duplicates on a bad timestamp would send somebody to review parts that are fine.
 */
export function justAdded(clusters: readonly DuplicateCluster[], since: string): number {
  const from = Date.parse(since)
  if (Number.isNaN(from)) return 0
  const added = new Set<PartId>()
  for (const cluster of clusters) {
    for (const part of cluster.parts) {
      if (at(part) >= from) added.add(part.id)
    }
  }
  return added.size
}
