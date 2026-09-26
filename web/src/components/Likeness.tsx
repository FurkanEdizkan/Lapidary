import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import {
  fetchDuplicates,
  fetchLikeness,
  foldPart,
  justAdded,
  newestFirst,
  setPartLink,
} from '../lib/likeness'
import { strings } from '../lib/strings'
import { arrive } from '../lib/motion'
import { Dialog } from './Dialog'
import { breakable } from './Card'
import type { BatchKind } from './Upload'
import type { BatchStatus, DuplicateCluster, LibraryId, PartCard } from '../lib/types'

/**
 * The part a decision is made *into*: the one whose page this is, or a group's first part.
 *
 * Narrower than `PartCard` on purpose, so the part page can pass its `PartDetail` — which is a
 * different wire type carrying the same two fields — without either side converting.
 */
type Kept = Pick<PartCard, 'id' | 'sourcePath'>

/**
 * Everything that says two parts look alike: the part page's section, the review queue's
 * groups, and the line a finished upload adds.
 *
 * One module, because all three draw the same row and offer the same three decisions, and the
 * rule they share is the one that must not drift: **a person reads a word, never a number.**
 * Identical, near-duplicate, similar. The wire types carry no score, so there is nothing to
 * leak — and no figure here is mesh-derived either, except a part's own triangle count, which
 * arrives already labelled the way every other count in the interface is.
 *
 * "Fold into", never "merge". `CLAUDE.md` keeps "merge" for versioning, which has none of it;
 * and a fold is the soft delete, so the folded part keeps its tags, sources, images and
 * revisions, nothing on disk moves, and Restore brings it back. The dialog says all three.
 */

const QUIET =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2.5 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * One part in a likeness list: its render, its name, its path, and its triangle count.
 *
 * The path is not decoration. Parts that look alike are very often parts that share a name —
 * that is half of why they look alike — so a row that shows only the name cannot be told from
 * the row above it, and the decision buttons beside it would be a coin toss.
 *
 * The triangle count carries `strings.parts.approximate` exactly as the grid card does, for the
 * reason `CLAUDE.md` makes non-negotiable: it is measured from tessellated geometry, so it is
 * labelled, always. It is the only figure on this row. The *judgement* behind the row has no
 * figure to label, because none is shown.
 */
function Alike({ card, children }: { card: PartCard; children?: ReactNode }) {
  const count = typeof card.triangleCount === 'number' ? card.triangleCount : null
  return (
    <li className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-[var(--color-border)] py-2.5">
      <div className="grid size-[46px] flex-none place-items-center overflow-hidden rounded-[var(--radius-ctl)] bg-[var(--color-raised)] p-1">
        {card.thumbnail === null ? null : (
          <img src={card.thumbnail} alt="" className="h-full w-full object-contain" />
        )}
      </div>
      <div className="min-w-0 grow">
        <Link
          to="/parts/$partId"
          params={{ partId: card.id }}
          className="ease-mechanical text-sm font-medium text-[var(--color-bright)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
        >
          {breakable(card.name)}
        </Link>
        <p className="tabular text-xs break-all text-[var(--color-muted)]">{card.sourcePath}</p>
        {count === null ? null : (
          <p className="tabular flex flex-wrap items-baseline gap-x-1.5 text-[11px] text-[var(--color-muted)]">
            <span>{strings.parts.triangles(count)}</span>
            {!card.approximate ? null : (
              <span
                title={strings.parts.approximateDetail}
                className="underline decoration-[var(--color-edge)] decoration-dotted underline-offset-2"
              >
                {strings.parts.approximate}
              </span>
            )}
          </p>
        )}
      </div>
      {children}
    </li>
  )
}

/**
 * The three decisions about one pair, and the only three there are.
 *
 * `keep` is the part these read "into this part" about: the part whose page this is, or the
 * first part of a queue group. `card` is the one the decision is about, and the one a fold
 * removes — which is why {@link foldPart} takes it first and the dialog names both by path.
 *
 * Every write invalidates four things. `['likeness']` as a bare prefix because the *other*
 * part's cached likeness changed too and this page does not know whose pages are cached; the
 * queue, which is computed from links; the folds list the Removed page reads; and
 * `['parts', library]`, which covers the grid and its `'removed'` child at once.
 */
function Decisions({ card, keep }: { card: PartCard; keep: Kept }) {
  const queryClient = useQueryClient()
  const [folding, setFolding] = useState(false)
  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['likeness'] }),
      queryClient.invalidateQueries({ queryKey: ['duplicates', card.library] }),
      queryClient.invalidateQueries({ queryKey: ['folds', card.library] }),
      queryClient.invalidateQueries({ queryKey: ['parts', card.library] }),
    ])
  }
  const fold = useMutation({ mutationFn: () => foldPart(card.id, keep.id), onSuccess: refresh })
  // The kind is chosen out here, not in the JSX: a `'variant'` inside a child expression is a
  // bare literal reaching the screen as far as `no-bare-strings.test.ts` can tell, and it is
  // right to say so — see the same note in `ScanProgress`.
  const link = useMutation({
    mutationFn: (kind: 'variant' | 'distinct') => setPartLink(keep.id, card.id, kind),
    onSuccess: refresh,
  })
  const busy = fold.isPending || link.isPending
  return (
    <div className="flex flex-none flex-wrap gap-2">
      {fold.isError || link.isError ? (
        <span role="alert" className="text-xs text-[var(--color-muted)]">
          {strings.likeness.actionFailed}
        </span>
      ) : null}
      <button
        type="button"
        aria-label={strings.likeness.foldLabel(card.sourcePath)}
        disabled={busy}
        onClick={() => setFolding(true)}
        className={QUIET}
      >
        {fold.isPending ? strings.likeness.working : strings.likeness.fold}
      </button>
      <button
        type="button"
        aria-label={strings.likeness.variantLabel(card.sourcePath)}
        disabled={busy}
        onClick={() => link.mutate('variant')}
        className={QUIET}
      >
        {strings.likeness.variant}
      </button>
      <button
        type="button"
        aria-label={strings.likeness.distinctLabel(card.sourcePath)}
        disabled={busy}
        onClick={() => link.mutate('distinct')}
        className={`${QUIET} text-[var(--color-muted)]`}
      >
        {strings.likeness.distinct}
      </button>
      {!folding ? null : (
        /*
          Confirmed, unlike Remove — which is reversible, changes nothing on disk, and says so
          beside the button. This one is reversible too, and the dialog exists anyway: a fold acts
          on a part that is not the one on screen, names two parts that usually share a name, and
          is the one action here somebody could aim at the wrong row. Cancel has focus, so a
          reflexive Enter keeps both parts.
        */
        <Dialog title={strings.likeness.foldTitle} onClose={() => setFolding(false)}>
          <p className="mt-3 text-sm">
            {strings.likeness.foldConfirm(card.sourcePath, keep.sourcePath)}
          </p>
          <div className="mt-4 flex justify-end gap-2">
            <button type="button" autoFocus onClick={() => setFolding(false)} className={QUIET}>
              {strings.folders.cancel}
            </button>
            <button
              type="button"
              onClick={() => {
                setFolding(false)
                fold.mutate()
              }}
              className={QUIET}
            >
              {strings.likeness.foldConfirmAction}
            </button>
          </div>
        </Dialog>
      )}
    </div>
  )
}

/** One titled list of alike parts, with or without the decisions. Nothing is drawn for an empty one. */
function Kind({
  title,
  note,
  cards,
  keep,
}: {
  title: string
  note: string
  cards: readonly PartCard[]
  /** The part the decisions act "into", or `null` for a list that offers none. */
  keep: Kept | null
}) {
  if (cards.length === 0) return null
  return (
    <div className="mt-5">
      <h4 className="text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">{title}</h4>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{note}</p>
      <ul role="list" className="mt-2 flex flex-col border-t border-[var(--color-border)]">
        {cards.map((card) => (
          <Alike key={card.id} card={card}>
            {keep === null ? null : <Decisions card={card} keep={keep} />}
          </Alike>
        ))}
      </ul>
    </div>
  )
}

/**
 * The part page's section: what looks like this part, in the three kinds `phase-6.md` fixes,
 * plus the variants somebody already decided about.
 *
 * One `<section>` with one `h3`, so the page's `SectionIndex` picks it up as a single link and
 * the four lists are `h4`s under it. Not built with `Section`, which wraps its children in a
 * `<dl>` — these are lists of parts, not label-and-value rows.
 *
 * **The decisions are offered on Identical and Near-duplicates only.** Those are the two kinds
 * that are open questions. A variant has already been decided, and "Similar" is the browse
 * list — top-k by distance, whatever the size — so writing a `distinct` from it would record a
 * decision about a pair nothing ever proposed.
 */
export function Likeness({ part }: { part: Kept }) {
  const likeness = useQuery({ queryKey: ['likeness', part.id], queryFn: () => fetchLikeness(part.id) })
  const lists = useRef<HTMLDivElement>(null)
  // The lists fade up in turn when they arrive, once per part. Not on a refetch of what is
  // already on screen: a decision re-reads this section, and re-animating it would say
  // something new had appeared when the only news is that a row left.
  useLayoutEffect(() => {
    const zone = lists.current
    if (zone === null || likeness.data === undefined) return
    return arrive(Array.from(zone.children).filter((node): node is HTMLElement => node instanceof HTMLElement))
  }, [part.id, likeness.data === undefined])

  const data = likeness.data
  const empty =
    data !== undefined &&
    data.identical.length === 0 &&
    data.nearDuplicates.length === 0 &&
    data.similar.length === 0 &&
    data.variants.length === 0

  return (
    <section id="part-likeness" className="mb-6 scroll-mt-20">
      <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {strings.likeness.title}
      </h3>
      {/* The error branch first: a failed read that also has no data must say so, not load forever. */}
      {likeness.isError ? (
        <p role="alert" className="max-w-prose text-sm text-[var(--color-muted)]">
          {strings.likeness.failed}
        </p>
      ) : data === undefined ? (
        <p className="text-sm text-[var(--color-muted)]">{strings.likeness.loading}</p>
      ) : !data.profiled ? (
        /*
          Said instead of the lists, and this is the distinction the section exists to keep: four
          empty lists read as "nothing here is alike", while the truth is that this part's shape
          has not been compared with anything yet. `identical` needs no profile and would be the
          only honest list — but shown alone under four headings it implies the other three were
          checked and came back empty.
        */
        <p className="max-w-prose text-sm text-[var(--color-muted)]">{strings.likeness.unprofiled}</p>
      ) : empty ? (
        <p className="max-w-prose text-sm text-[var(--color-muted)]">{strings.likeness.nothing}</p>
      ) : (
        <div ref={lists}>
          <Kind
            title={strings.likeness.identical}
            note={strings.likeness.identicalNote}
            cards={data.identical}
            keep={part}
          />
          <Kind
            title={strings.likeness.nearDuplicates}
            note={strings.likeness.nearNote}
            cards={data.nearDuplicates}
            keep={part}
          />
          <Kind
            title={strings.likeness.variants}
            note={strings.likeness.variantsNote}
            cards={data.variants}
            keep={null}
          />
          <Kind
            title={strings.likeness.similar}
            note={strings.likeness.similarNote}
            cards={data.similar}
            keep={null}
          />
        </div>
      )}
    </section>
  )
}

/**
 * One group in the review queue: the part it keeps, and every part that looks like it.
 *
 * The group's first part is the largest — that is the order the wire sends — and it is what the
 * decisions act "into". So it is named in the heading and drawn without buttons, and the rest
 * carry them. A group of two is the ordinary case and reads as a pair.
 */
function Cluster({ cluster }: { cluster: DuplicateCluster }) {
  const [keep, ...rest] = cluster.parts
  if (keep === undefined) return null
  return (
    <li className="mt-6 border-t border-[var(--color-border)] pt-3 first:mt-0">
      <div className="flex flex-wrap items-baseline gap-x-3">
        <h3 className="text-sm font-medium text-[var(--color-bright)]">
          {strings.likeness.clusterKept(keep.sourcePath)}
        </h3>
        <span className="text-xs text-[var(--color-muted)]">
          {cluster.identical ? strings.likeness.sameBytes : strings.likeness.alikeShape}
        </span>
      </div>
      <ul role="list" className="mt-2 flex flex-col border-t border-[var(--color-border)]">
        <Alike card={keep} />
        {rest.map((card) => (
          <Alike key={card.id} card={card}>
            <Decisions card={card} keep={keep} />
          </Alike>
        ))}
      </ul>
    </li>
  )
}

/**
 * The review queue's body: the groups, newest first, and what could not be compared.
 *
 * `unprofiled` is said whether or not there are groups. A page that lists nothing and stops
 * there claims the library was checked in full; a library where half the parts have no profile
 * yet has not been.
 */
export function Clusters({ library, since }: { library: LibraryId; since?: string }) {
  const duplicates = useQuery({
    queryKey: ['duplicates', library, since],
    queryFn: () => fetchDuplicates(library, since),
  })
  if (duplicates.isPending) {
    return <p className="mt-6 text-[var(--color-muted)]">{strings.likeness.loading}</p>
  }
  if (duplicates.isError) {
    return (
      <p role="alert" className="mt-6 max-w-prose text-[var(--color-muted)]">
        {strings.likeness.failed}
      </p>
    )
  }
  const groups = newestFirst(duplicates.data.clusters)
  const unprofiled = duplicates.data.unprofiled
  return (
    <>
      <p className="mt-6 text-sm text-[var(--color-muted)]">
        {groups.length === 0 ? strings.likeness.queueEmpty : strings.likeness.queueCount(groups.length)}
      </p>
      {unprofiled === 0 ? null : (
        <p className="mt-1 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.likeness.queueUnprofiled(unprofiled)}
        </p>
      )}
      {groups.length === 0 ? null : (
        <ul role="list" className="mt-3">
          {groups.map((cluster) => (
            <Cluster key={cluster.parts[0]?.id} cluster={cluster} />
          ))}
        </ul>
      )}
    </>
  )
}

/**
 * The line a finished upload or scan adds: how many of the parts just added look like others,
 * and a way to look at them.
 *
 * **Gated twice, and the second gate is the one that matters.** It waits for `finishedAt`,
 * because a batch still running has parts with no profile yet; and it only ever asks for a
 * scan or an upload, because a thumbnail sweep and a storage migration add no parts at all and
 * a duplicates query on the end of either is a query nobody asked for. The gate is the query's
 * `enabled` rather than an early return, so nothing is fetched when the line will not show.
 *
 * `since` is the batch's own start, so the queue it links to is this batch's and not the whole
 * library's — after eleven files, the library's queue is the wrong page to land on.
 */
export function ReviewOffer({
  status,
  kind,
  library,
}: {
  status?: BatchStatus
  kind: BatchKind
  library: LibraryId
}) {
  const since = typeof status?.startedAt === 'string' ? status.startedAt : undefined
  const settled = status?.finishedAt != null && (kind === 'upload' || kind === 'scan')
  const duplicates = useQuery({
    queryKey: ['duplicates', library, since],
    queryFn: () => fetchDuplicates(library, since),
    enabled: settled && since !== undefined,
  })
  const count =
    duplicates.data === undefined || since === undefined ? 0 : justAdded(duplicates.data.clusters, since)
  if (!settled || count === 0) return null
  return (
    <p className="mb-4 flex flex-wrap items-center gap-2 text-sm text-[var(--color-muted)]">
      <span>{strings.likeness.reviewOffer(count)}</span>
      <Link to="/duplicates" search={{ library, since }} className={QUIET}>
        {strings.likeness.review}
      </Link>
    </p>
  )
}
