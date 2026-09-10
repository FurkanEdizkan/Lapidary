import { useQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { fetchLibraries } from '../lib/api'
import { strings } from '../lib/strings'
import { FolderTree } from './FolderTree'
import type { FolderId, InstanceStorageView, LibraryId } from '../lib/types'

/**
 * The rail down the left of the application — `v2`'s sidebar.
 *
 * # What it holds, and what the design holds that it does not
 *
 * The design's rail runs LIBRARIES, FOLDERS, TAGS, COLLECTIONS, FILTERS and a footer. Three
 * of those six are shipped here, and the three that are missing are missing because there
 * is nothing behind them: there is no tag table, no collection table, and no facet index
 * for a format or a process filter — `ROADMAP.md` puts faceted filters in Phase 2 and tags
 * in Phase 5. A heading over an empty list is worse than no heading; it reads as a library
 * with no tags rather than as an application that cannot store one.
 *
 * The design's max-height slider is left out for a sharper reason than "no backend". It
 * *could* be built today by filtering the cards in hand — and it would lie. The grid is
 * keyset-paged, so what is in hand is the first 50 parts of an arbitrary number; a slider
 * that hides four of them and leaves the other 1,700 unexamined would report "12 parts
 * under 80 mm" about a library holding two hundred. A filter belongs in the query or
 * nowhere, and putting it in the query means a facet index. See `strings.parts.height` for
 * the column that does ship, which states one part's height rather than filtering on it.
 *
 * # Scrolling
 *
 * The rail scrolls independently of the grid and the footer does not scroll with it. A
 * storage figure that leaves the screen when a person scrolls a long category tree is a
 * figure they have to go looking for, and it is the one number that describes the whole
 * library rather than any part of it.
 */
export function Sidebar({
  library,
  onSelectLibrary,
  folder,
  onSelectFolder,
  instance,
  partsLoaded,
  children,
}: {
  library: LibraryId
  onSelectLibrary?: (library: LibraryId) => void
  folder: FolderId | null
  onSelectFolder: (folder: FolderId | null) => void
  /** What the whole store holds. Read by the footer's bar; `undefined` while it loads. */
  instance?: InstanceStorageView
  /**
   * How many cards the grid has actually got. The footer states this rather than a library
   * total, because no route returns one: the grid is keyset-paged and the server does not
   * count a library to answer a page. See `strings.parts.showingSoFar`.
   */
  partsLoaded: number
  /** Anything the route wants under the tree — the "New library" control, in practice. */
  children?: ReactNode
}) {
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  const all = libraries.data ?? []

  return (
    <aside className="flex w-[228px] flex-none flex-col border-r border-[var(--color-border)] bg-[var(--color-raised)]">
      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-[10px] pt-[14px] pb-[22px]">
        {/*
          The library list, and it is a list of *rows* rather than a `<select>`.

          The switcher this replaces hid itself entirely below two libraries, on the
          argument that a control offering one choice explains nothing. A row does explain
          something a select cannot: it carries the part count, which is the figure that
          tells a person which of two similarly named libraries is the one they filled. So
          it renders at one library too — where it reads as a label with a count, which is
          true and useful — and the reason the old control hid does not apply to it.
        */}
        <Section title={strings.libraries.heading}>
          {libraries.isError ? (
            <p role="alert" className="px-2 text-xs text-[var(--color-muted)]">
              {strings.libraries.failed}
            </p>
          ) : (
            <ul role="list" className="flex flex-col gap-[3px]">
              {all.map((one) => (
                <li key={one.id}>
                  <button
                    type="button"
                    onClick={() => onSelectLibrary?.(one.id)}
                    /*
                      `aria-current="true"` and not `page`: this does not navigate to a
                      different page, it changes which library every query on the page is
                      about. `page` would be a claim the URL does not support — the route is
                      the same one either way.
                    */
                    aria-current={one.id === library ? 'true' : undefined}
                    className={`ease-mechanical flex w-full items-center gap-2 rounded-[var(--radius-ctl)] px-2 py-[7px] text-left duration-[var(--duration-fast)] hover:bg-[var(--color-surface)] ${
                      one.id === library
                        ? 'bg-[var(--color-surface)] text-[var(--color-bright)]'
                        : 'text-[var(--color-text)]'
                    }`}
                  >
                    <span className="min-w-0 flex-1 truncate text-[12.5px] font-semibold">
                      {one.name}
                    </span>
                    {/*
                      The count alone, not "1,204 models". The heading above says these are
                      libraries and the column of figures reads as counts; the word would
                      repeat on every row and push the name it belongs to into an ellipsis.

                      `aria-label` puts the word back for a screen reader, which gets no
                      column and no heading beside the figure — the row would otherwise
                      announce a library name followed by a bare number.
                    */}
                    <span
                      aria-label={strings.libraries.partsIn(one.partCount)}
                      className="tabular flex-none text-[11px] text-[var(--color-muted)]"
                    >
                      {strings.libraries.count(one.partCount)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {children}
        </Section>

        {/*
          The category tree, which brings its own heading and its own "New category"
          control. Not wrapped in `Section`: it is a `nav` with an `h2` already, and putting
          a second heading over it would announce the region twice.
        */}
        <FolderTree library={library} selected={folder} onSelect={onSelectFolder} />
      </div>

      {/*
        The foot of the rail: what the grid has, and what the store holds. Outside the
        scrolling region, so it stays put while a long tree scrolls past it.
      */}
      <div className="flex flex-none flex-col gap-[6px] border-t border-[var(--color-border)] px-[14px] py-[10px]">
        {/*
          The count on its own, not `parts.showingSoFar` — which is the sentence the scope
          line above the grid already renders, and two elements carrying identical text is
          two things for a screen reader to read and one for a test to pick between. This
          says the same fact in the rail's own register: a figure and a noun, uppercased.
        */}
        <p className="tabular text-[9.5px] tracking-wider text-[var(--color-muted)] uppercase">
          {strings.libraries.partsIn(partsLoaded)}
        </p>
        <PoolBar instance={instance} />
      </div>
    </aside>
  )
}

/**
 * A labelled block in the rail. The heading is the design's mono all-caps eyebrow, and it
 * is an `h2` rather than a styled `div`: the rail is a landmark with three regions in it,
 * and a screen reader's heading list is how somebody skips between them.
 */
function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-[3px]">
      <h2 className="tabular px-2 pb-[6px] text-[9px] tracking-[0.22em] text-[var(--color-muted)] uppercase">
        {title}
      </h2>
      {children}
    </section>
  )
}

/**
 * The store's occupancy, as one bar and one line.
 *
 * # It reports a proportion, not a quota
 *
 * The design's bar fills against a fixed pool — `812 GB of 2,048 GB` — which needs a quota,
 * and Lapidary has none: the store is a volume somebody mounted, and no route reports its
 * capacity. So the bar shows the split this application *does* know, which is the one that
 * answers a real question: how much of what is on disk is source geometry versus the
 * derivatives that could be regenerated. That is the figure Phase 4's tiering work acts on,
 * and it is the difference between "I am out of space" and "I am out of space because of
 * things I can rebuild".
 *
 * Nothing renders until the read lands. A bar drawn at zero while a fetch is in flight is a
 * measurement of an empty store, which is a different fact from not knowing yet — and
 * `CLAUDE.md` treats a figure that states something untrue as a correctness bug rather than
 * a cosmetic one.
 */
function PoolBar({ instance }: { instance?: InstanceStorageView }) {
  if (instance === undefined) return null
  const source = instance.sourceBytes
  const derivative = instance.derivativeBytes + instance.inlinePreviewBytes
  const total = source + derivative
  if (total === 0) return null
  /*
    Rounded here and used for both halves, so the two widths always sum to 100 and the bar
    has no seam of background showing through at an arbitrary ratio.
  */
  const sourcePercent = Math.round((source / total) * 100)
  return (
    <>
      <div
        /*
          `role="img"` with a label, not a `progressbar`. A progress bar announces a value
          against a maximum, and there is no maximum here — the label is the whole of what
          this says, and the bar is the picture of it.
        */
        role="img"
        aria-label={strings.storage.poolSplit(source, derivative)}
        className="flex h-[4px] overflow-hidden rounded-[3px] bg-[var(--color-border)]"
      >
        <span
          style={{ width: `${sourcePercent}%` }}
          className="block bg-[var(--color-accent)]"
        />
        <span className="block flex-1 bg-[var(--color-good)]" />
      </div>
      <p aria-hidden="true" className="tabular text-[9px] text-[var(--color-muted)]">
        {strings.storage.poolSplit(source, derivative)}
      </p>
    </>
  )
}
