import { Link } from '@tanstack/react-router'
import { strings } from '../../lib/strings'
import { breakable, pictureStyle } from '../Card'
import { StorageTotals } from '../Storage'
import type {
  FacetKind,
  FacetValue,
  InstanceStorageView,
  LibraryStorage,
  PartCard,
} from '../../lib/types'
import type { QueueSummary } from '../../lib/types'

/**
 * What each of the six widgets draws, once its value has arrived.
 *
 * One module rather than a file per kind: every one of these is a paragraph or a short list,
 * and six files of eight lines would put the shared row — a picture, a name, a path — in
 * whichever of them was written first and import it sideways.
 *
 * Nothing here fetches. Every value on this page came out of the one resolve, and a widget
 * that read anything of its own would be the per-widget request `FEATURES.md` §8 refuses.
 */

/** The body text every widget's figures are set in. */
const FIGURE = 'text-xs text-[var(--color-muted)]'

/**
 * What a library holds, from the panel the grid already draws it with.
 *
 * Reused rather than rewritten: this is the same claim about the same library, and two
 * renderings of `LibraryStorage` would be two chances to divide the ratio the other way up —
 * which is the thing `LibraryStorage.derivativeRatio`'s own doc exists to prevent.
 */
export function StorageBody({ value }: { value: LibraryStorage }) {
  return <StorageTotals storage={value} isError={false} />
}

/**
 * What the whole store holds, in the one sentence that can name the bytes no per-library
 * figure admits to — quarantined ones belong to no library, because whatever said which one is
 * gone: since R4's `0055`, a render derivative Lapidary deleted itself.
 */
export function InstanceStorageBody({ value }: { value: InstanceStorageView }) {
  return (
    <p className={`${FIGURE} max-w-prose`}>
      {strings.storage.everything(
        value.sourceBytes,
        value.derivativeBytes,
        value.quarantinedBytes,
      )}
    </p>
  )
}

/**
 * A short list of parts: what "recently added" and a saved filter both are.
 *
 * No triangle count and no figures. The grid's card carries them with the `approximate` mark
 * beside each one, and a widget four rows tall has room for a name or for a number but not for
 * both with the label that keeps the number honest — so it shows the name, and the part's own
 * page shows the figures with their mark. `CLAUDE.md`'s rule is that a mesh-derived figure is
 * always labelled; the way to keep it here is not to print one.
 */
export function PartsBody({ parts }: { parts: readonly PartCard[] }) {
  if (parts.length === 0) {
    return <p className={FIGURE}>{strings.dashboard.noParts}</p>
  }
  return (
    <ul role="list" className="flex flex-col">
      {parts.map((part) => (
        <li key={part.id} className="flex items-center gap-2.5 border-b border-[var(--color-border)] py-1.5 last:border-b-0">
          <span className="grid size-8 flex-none place-items-center overflow-hidden rounded-[3px] bg-[var(--color-raised)] p-[2px]">
            {/* The part's own picture, or nothing: there is no render behind it since R3. */}
            {part.image === null ? null : (
              <img
                src={part.image.src}
                alt=""
                className="h-full w-full"
                style={pictureStyle(part.image)}
              />
            )}
          </span>
          <span className="min-w-0 grow">
            <Link
              to="/parts/$partId"
              params={{ partId: part.id }}
              className="ease-mechanical block truncate text-xs font-medium text-[var(--color-bright)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
            >
              {breakable(part.name)}
            </Link>
          </span>
        </li>
      ))}
    </ul>
  )
}

/**
 * One value and how many parts carry it.
 *
 * A format is shown upper-cased and a material or tag as it was written, which is the same
 * distinction the grid's facet rail makes — a format is an extension and a tag is somebody's
 * word. `count` is `null` past the exact-count threshold, where the wire says which values
 * occur and not how often.
 */
export function FacetBody({ values, facet }: { values: readonly FacetValue[]; facet: FacetKind }) {
  if (values.length === 0) {
    return <p className={FIGURE}>{strings.dashboard.noValues}</p>
  }
  // Decided here rather than inside the JSX: the bare-strings gate reports any literal written
  // in a child expression, technical ones included, and it is right to — that is how an inlined
  // formatter slips past it.
  const upperCased = facet === 'format'
  return (
    <ul role="list" className="flex flex-col">
      {values.map((entry) => (
        <li
          key={entry.value}
          className="flex items-baseline justify-between gap-3 border-b border-[var(--color-border)] py-1 text-xs last:border-b-0"
        >
          <span className="min-w-0 truncate text-[var(--color-text)]">
            {upperCased ? strings.facets.name(entry.value) : entry.value}
          </span>
          <span className="tabular flex-none text-[var(--color-muted)]">
            {strings.dashboard.facetTally(entry.count)}
          </span>
        </li>
      ))}
    </ul>
  )
}

/** What the worker still owes this library. Only the clauses that are true. */
export function QueueBody({ value }: { value: QueueSummary }) {
  return <p className={FIGURE}>{strings.dashboard.queueLine(value.pending, value.running, value.failed)}</p>
}

