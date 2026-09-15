import { annotationsOf, labelsFor } from '../lib/annotations'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Fragment, Suspense, lazy, useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import {
  addPartSource,
  blobUrl,
  downloadUrl,
  exportUrl,
  fetchBatchStatus,
  fetchDiff,
  fetchEntities,
  fetchFields,
  fetchLibraries,
  fetchPartImages,
  fetchPartSources,
  fetchPmi,
  fetchRevisions,
  fetchStructure,
  openLink,
  releaseLock,
  requestExport,
  setFieldValue,
  setImageFraming,
  setPartTags,
  uploadPartImage,
} from '../lib/api'
import { strings } from '../lib/strings'
import { Dialog } from './Dialog'
import { Figure } from './Figure'
import { hasWebGL } from '../lib/viewer-math'
import type {
  AssemblyNode,
  BatchId,
  BlobHash,
  CustomField,
  Delta,
  PartDetail as PartDetailData,
  PartId,
  PartImage,
  PartLock,
  PartRevision,
  PmiFace,
  RevisionId,
} from '../lib/types'

/**
 * One picture in the gallery, framed the way its row says — and adjustable in place.
 *
 * **The browser does the framing.** `object-fit` and `object-position` are exactly the two
 * things stored on the row, so nothing here computes a crop, no canvas is involved, and the
 * stored WebP is never re-encoded. Adjusting is free, reversible, and costs the picture
 * nothing — which is the whole reason the framing is data rather than baked into the bytes.
 *
 * The focal point is set by clicking the picture, because that is the gesture: point at the
 * part and it stays in frame when the tile crops. The keyboard gets the same thing through
 * the arrow keys, since a click target is not an interaction everybody has.
 */
function Framed({ part, image, label }: { part: PartId; image: PartImage; label: string }) {
  const queryClient = useQueryClient()
  const reframe = useMutation({
    mutationFn: (framing: { fit: string; focusX: number; focusY: number }) =>
      setImageFraming(part, image.id, framing),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['part-images', part] }),
  })

  /** Clamped, because the server refuses anything outside 0–1 and a rounding error is not
      a reason to lose an adjustment. */
  const clamp = (value: number) => Math.min(1, Math.max(0, value))
  const move = (dx: number, dy: number) =>
    reframe.mutate({
      fit: image.fit,
      focusX: clamp(image.focusX + dx),
      focusY: clamp(image.focusY + dy),
    })

  // Named above the JSX rather than compared inside it: `no-bare-strings.test.ts` reads a
  // string literal in a JSX child as a bare user-facing string, and it is right to — the
  // difference between a label and a comparison operand is not visible in the tree it walks.
  const filling = image.fit !== 'contain'

  const nudge: Record<string, [number, number]> = {
    ArrowLeft: [-0.1, 0],
    ArrowRight: [0.1, 0],
    ArrowUp: [0, -0.1],
    ArrowDown: [0, 0.1],
  }

  return (
    <div className="flex flex-col items-center gap-1">
      {/*
        A `<button>` and not a bare `<img>` with a handler: this is a control, it is reached
        by Tab, and the browser's own focus ring is worth more than a div with a role.
      */}
      <button
        type="button"
        aria-label={strings.images.focusLabel(label)}
        onClick={(event) => {
          const box = event.currentTarget.getBoundingClientRect()
          reframe.mutate({
            fit: image.fit,
            focusX: clamp((event.clientX - box.left) / box.width),
            focusY: clamp((event.clientY - box.top) / box.height),
          })
        }}
        onKeyDown={(event) => {
          const step = nudge[event.key]
          if (step === undefined) return
          event.preventDefault()
          move(step[0], step[1])
        }}
        className="block h-24 w-24 overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]"
      >
        <img
          src={image.src}
          alt={label}
          title={image.sourceUrl === null ? undefined : strings.images.from(image.sourceUrl)}
          className="h-full w-full"
          style={{
            objectFit: filling ? 'cover' : 'contain',
            objectPosition: `${image.focusX * 100}% ${image.focusY * 100}%`,
          }}
        />
      </button>
      {/*
        Two states, so a toggle rather than a pair of radios. `cover` fills the tile and
        loses the edges; `contain` shows the whole picture and letterboxes it. Which one is
        right depends on the picture, which is why this is a control and not a constant.
      */}
      <button
        type="button"
        onClick={() =>
          reframe.mutate({
            fit: filling ? 'contain' : 'cover',
            focusX: image.focusX,
            focusY: image.focusY,
          })
        }
        disabled={reframe.isPending}
        className="ease-mechanical rounded px-1 text-[10px] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:underline disabled:opacity-50"
      >
        {filling ? strings.images.fitWhole : strings.images.fitFill}
      </button>
      {reframe.isError ? (
        <p role="alert" className="w-24 text-center text-[10px] leading-snug text-[var(--color-muted)]">
          {strings.images.reframeFailed}
        </p>
      ) : null}
    </div>
  )
}

/**
 * Where the part came from: the link, the vendor, the price, and the licence.
 *
 * **The licence is the field this section exists for.** `docs/DATA.md`: half of hobbyist STL
 * libraries are non-commercial, and somebody selling prints needs to see that before they
 * print. It is here and not on the grid card because the grid query is at its column ceiling
 * — a decision recorded in `crates/lapidary-api/src/sources.rs`, not an omission.
 *
 * Nothing here fetches anything. Pasting a product page records the link; the application
 * has exactly one route that makes an outbound request and this is not it.
 */
function Sources({ part, recordable }: { part: PartId; recordable: boolean }) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [refusal, setRefusal] = useState<string | null>(null)

  const sources = useQuery({
    queryKey: ['part-sources', part],
    queryFn: () => fetchPartSources(part),
  })

  const add = useMutation({
    mutationFn: (form: FormData) => {
      const text = (field: string) => {
        const value = form.get(field)
        return typeof value === 'string' && value.trim() !== '' ? value.trim() : null
      }
      // Money arrives as a decimal because that is how a price is written, and leaves as
      // minor units because that is how it is stored. `Math.round` and not a cast: 12.34
      // times 100 is 1233.9999999999998 in binary floating point, and a price that rounds
      // down by a penny on the way in is a bug nobody would find.
      const price = text('price')
      const priceMinor = price === null ? null : Math.round(Number(price) * 100)
      return addPartSource(part, {
        url: text('url'),
        vendor: text('vendor'),
        externalId: text('externalId'),
        title: text('title'),
        license: text('license'),
        priceMinor: priceMinor !== null && Number.isFinite(priceMinor) ? priceMinor : null,
        currency: text('currency'),
      })
    },
    onMutate: () => setRefusal(null),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setRefusal(result.message)
        return
      }
      setOpen(false)
      void queryClient.invalidateQueries({ queryKey: ['part-sources', part] })
    },
  })

  const recorded = sources.data ?? []
  // Nothing recorded and no way to record it here: a heading over an empty space says less
  // than the space it takes. The full page still offers the button.
  //
  // `isError` guards the early return as well as the list below it, because `?? []` reads
  // a failed fetch as a part with nothing recorded — and on a non-recordable part that
  // would take the whole section off the page. The licence this exists to show would be
  // missing with nothing on screen saying so, which is the one failure mode it has.
  if (!sources.isError && recorded.length === 0 && !recordable) return null
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.sources.title}
      </h3>
      {sources.isError ? (
        <p role="alert" className="mb-2 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.sources.failed}
        </p>
      ) : null}
      {recorded.length === 0 ? null : (
        <ul role="list" className="mb-2 list-none space-y-2">
          {recorded.map((source) => (
            <li
              key={source.id}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm"
            >
              <p>
                {source.url === null ? (
                  (source.title ?? source.vendor ?? strings.sources.untitled)
                ) : (
                  /*
                    `noreferrer` as well as `noopener`: this is a link somebody else's page
                    put in front of us, and the address of a private parts library is not
                    something to hand to it.
                  */
                  <a
                    href={source.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="underline"
                  >
                    {source.title ?? source.vendor ?? source.url}
                  </a>
                )}
              </p>
              <p className="mt-1 text-xs text-[var(--color-muted)]">
                {[
                  source.vendor,
                  source.externalId,
                  source.license,
                  source.priceMinor === null || source.currency === null
                    ? null
                    : strings.sources.price(source.priceMinor, source.currency),
                ]
                  .filter((part) => part !== null)
                  .join(' · ')}
              </p>
            </li>
          ))}
        </ul>
      )}
      {!recordable ? null : open ? (
        <form
          className="max-w-lg space-y-2"
          onSubmit={(event) => {
            event.preventDefault()
            add.mutate(new FormData(event.currentTarget))
          }}
        >
          {SOURCE_FIELDS.map((field) => (
            <label key={field.name} className="block text-xs text-[var(--color-muted)]">
              {field.label}
              <input
                name={field.name}
                type={field.type}
                step={field.type === 'number' ? '0.01' : undefined}
                className="mt-0.5 block w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-1 text-sm focus:border-[var(--color-accent)]"
              />
            </label>
          ))}
          <div className="flex items-center gap-2">
            <button
              type="submit"
              disabled={add.isPending}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
            >
              {add.isPending ? strings.sources.saving : strings.sources.save}
            </button>
            <button
              type="button"
              onClick={() => setOpen(false)}
              className="rounded px-2 py-1 text-xs text-[var(--color-muted)]"
            >
              {strings.sources.cancel}
            </button>
          </div>
        </form>
      ) : (
        <button
          type="button"
          onClick={() => {
            setRefusal(null)
            setOpen(true)
          }}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.sources.add}
        </button>
      )}
      {refusal === null ? null : (
        <p role="alert" className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {refusal}
        </p>
      )}
    </section>
  )
}

/**
 * The form's fields, as data. A table rather than eight hand-written `<label>`s: they differ
 * only in a name, a label and an input type, and eight copies of the same markup is eight
 * places for the class list to drift.
 */
const SOURCE_FIELDS = [
  { name: 'url', label: strings.sources.url, type: 'url' },
  { name: 'title', label: strings.sources.titleField, type: 'text' },
  { name: 'vendor', label: strings.sources.vendor, type: 'text' },
  { name: 'externalId', label: strings.sources.externalId, type: 'text' },
  { name: 'license', label: strings.sources.license, type: 'text' },
  { name: 'price', label: strings.sources.priceField, type: 'number' },
  { name: 'currency', label: strings.sources.currency, type: 'text' },
] as const

/**
 * The part, rendered whole. Exported because the grid's quick-look shows exactly this and
 * must not render its own version of it.
 *
 * Two renderings of one measurement that can disagree is what this repository treats as a
 * defect, and measurements are the case that matters most: every figure goes through
 * `Figure`, which cannot render a value without its `approximate` flag, because `CLAUDE.md`
 * requires a mesh-derived number to be labelled wherever it appears. A dialog with its own
 * `<dl>` would be one refactor away from dropping that.
 */
/**
 * The tags a person gave the part. Listed wherever the part is shown, and edited only where
 * `recordable` is on, for the reason `Detail` gives: a tag half-typed into a dialog that closes on
 * Escape is a tag somebody loses.
 *
 * Every change sends the whole list, which is what the route takes, and the part is read again
 * afterwards, so the page shows what the server kept rather than what was typed.
 */
function Tags({ part, recordable }: { part: PartDetailData; recordable: boolean }) {
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState('')
  const [refusal, setRefusal] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: (tags: readonly string[]) => setPartTags(part.id, tags),
    onMutate: () => setRefusal(null),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setRefusal(result.message)
        return
      }
      setDraft('')
      void queryClient.invalidateQueries({ queryKey: ['part', part.id] })
    },
  })
  // `?? []` for a server from before tags, which sends a part without them.
  const tags = part.tags ?? []
  if (tags.length === 0 && !recordable) return null
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.tags.title}
      </h3>
      {tags.length === 0 ? null : (
        <ul role="list" className="mb-2 flex list-none flex-wrap gap-1">
          {tags.map((tag) => (
            <li
              key={tag}
              className="flex items-center gap-1 rounded-sm border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-sm"
            >
              {tag}
              {recordable ? (
                <button
                  type="button"
                  aria-label={strings.tags.remove(tag)}
                  disabled={save.isPending}
                  onClick={() => save.mutate(tags.filter((kept) => kept !== tag))}
                  className="text-xs text-[var(--color-muted)] hover:text-[var(--color-bright)] disabled:opacity-50"
                >
                  {strings.glyphs.remove}
                </button>
              ) : null}
            </li>
          ))}
        </ul>
      )}
      {recordable ? (
        <form
          className="flex max-w-sm items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (draft.trim() !== '') save.mutate([...tags, draft])
          }}
        >
          <label className="block flex-1 text-xs text-[var(--color-muted)]">
            {strings.tags.field}
            <input
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              className="mt-0.5 block w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-1 text-sm focus:border-[var(--color-accent)]"
            />
          </label>
          <button
            type="submit"
            disabled={save.isPending}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {save.isPending ? strings.tags.saving : strings.tags.add}
          </button>
        </form>
      ) : null}
      {refusal === null ? null : (
        <p role="alert" className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {refusal}
        </p>
      )}
    </section>
  )
}

/**
 * The part's own values for its library's custom fields (`docs/DATA.md` §3.5). Edited where tags
 * are, one value at a time. A value whose field the library has since removed is still the part's,
 * and is listed read-only under its key.
 */
function Fields({ part, recordable }: { part: PartDetailData; recordable: boolean }) {
  const fields = useQuery({
    queryKey: ['fields', part.library],
    queryFn: () => fetchFields(part.library),
  })
  // `?? {}` for a server from before custom fields, which sends a part without them.
  const custom: Record<string, unknown> = part.custom ?? {}
  if (fields.data === undefined) return null
  const defined = new Set(fields.data.map((field) => field.key))
  const orphaned = Object.entries(custom).filter(([key]) => !defined.has(key))
  const shown = recordable
    ? fields.data
    : fields.data.filter((field) => custom[field.key] !== undefined)
  if (shown.length === 0 && orphaned.length === 0) return null
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.fields.title}
      </h3>
      {shown.length === 0 ? null : (
        <dl className="grid max-w-sm grid-cols-[auto_1fr] items-baseline gap-x-3 gap-y-1 text-sm">
          {shown.map((field) => (
            <FieldValue
              key={`${part.id}:${field.key}`}
              part={part}
              field={field}
              value={custom[field.key]}
              recordable={recordable}
            />
          ))}
        </dl>
      )}
      {orphaned.length === 0 ? null : (
        <>
          <h4 className="mt-3 mb-1 text-xs text-[var(--color-muted)]">{strings.fields.orphaned}</h4>
          <dl className="grid max-w-sm grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm text-[var(--color-muted)]">
            {orphaned.map(([key, value]) => (
              <Fragment key={key}>
                <dt className="font-mono">{key}</dt>
                <dd>{String(value)}</dd>
              </Fragment>
            ))}
          </dl>
        </>
      )}
    </section>
  )
}

/**
 * One field's value. A number box sends what was typed when it is not a number, so the refusal is the
 * server's own sentence naming the field rather than a second copy of its rule here.
 */
function FieldValue({
  part,
  field,
  value,
  recordable,
}: {
  part: PartDetailData
  field: CustomField
  value: unknown
  recordable: boolean
}) {
  const queryClient = useQueryClient()
  const stored = value === undefined || value === null ? '' : String(value)
  const [draft, setDraft] = useState(stored)
  // A value read back changed replaces the draft, so a blur never writes the old one over it. Set while
  // rendering rather than by a key on the stored value, which would remount the input and drop its focus
  // after every save.
  const [seen, setSeen] = useState(stored)
  if (seen !== stored) {
    setSeen(stored)
    setDraft(stored)
  }
  const [refusal, setRefusal] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: (next: string | number | null) => setFieldValue(part.id, field.key, next),
    onMutate: () => setRefusal(null),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setRefusal(result.message)
        return
      }
      void queryClient.invalidateQueries({ queryKey: ['part', part.id] })
    },
  })
  const shown = stored === '' ? strings.fields.unset : stored
  if (!recordable) {
    return (
      <>
        <dt className="text-[var(--color-muted)]">{field.label}</dt>
        <dd>{shown}</dd>
      </>
    )
  }
  const commit = (raw: string) => {
    const trimmed = raw.trim()
    if (trimmed === stored) return
    if (trimmed === '') {
      save.mutate(null)
      return
    }
    const number = Number(trimmed)
    save.mutate(field.kind === 'number' && Number.isFinite(number) ? number : trimmed)
  }
  const id = `field-${field.key}`
  const choice = field.kind === 'choice'
  return (
    <>
      <dt>
        <label htmlFor={id} className="text-[var(--color-muted)]">
          {field.label}
        </label>
      </dt>
      <dd>
        {choice ? (
          <select
            id={id}
            value={draft}
            disabled={save.isPending}
            onChange={(event) => {
              setDraft(event.target.value)
              commit(event.target.value)
            }}
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-1 text-sm"
          >
            <option value="">{strings.fields.unset}</option>
            {field.options.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        ) : (
          <input
            id={id}
            value={draft}
            disabled={save.isPending}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={() => commit(draft)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') commit(draft)
            }}
            className="w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-1 text-sm focus:border-[var(--color-accent)]"
          />
        )}
        {refusal === null ? null : (
          <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
            {refusal}
          </p>
        )}
      </dd>
    </>
  )
}

/** Loaded only where a browser can draw it: three.js lives in this chunk and nowhere else. */
const loadViewer = () => import('./Viewer')
const Viewer = lazy(loadViewer)

/**
 * Fetch the viewer's chunk and compile its shaders ahead of an open; the grid calls this on hover
 * (`DATA.md` §2.4). Nothing happens where the browser cannot draw, and a failure is left for the
 * open itself to meet and report.
 */
export function warmViewer(): Promise<void> {
  if (!hasWebGL()) return Promise.resolve()
  return loadViewer()
    .then((viewer) => viewer.prepare())
    .catch(() => undefined)
}

/**
 * `warmViewer` once the browser is idle, for a screen a part can be opened from with no hover first:
 * a tap on a touch screen, or a press before the pointer rested. Returns what calls it off.
 * `setTimeout` stands in where there is no `requestIdleCallback`.
 */
export function warmViewerWhenIdle(): () => void {
  if (typeof requestIdleCallback === 'function') {
    // The pair taken together: the call-off can run after whoever supplied the request is gone.
    const cancel = cancelIdleCallback.bind(globalThis)
    const id = requestIdleCallback(() => void warmViewer(), { timeout: 2000 })
    return () => cancel(id)
  }
  const id = setTimeout(() => void warmViewer(), 1)
  return () => clearTimeout(id)
}

const FRAME =
  'relative h-40 w-40 overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]'

/**
 * The part's picture. The 3D view where the browser can draw one and the part has a rung to draw;
 * the rendered thumbnail otherwise, and as the poster the view replaces once its first frame is
 * drawn. The thumbnail is the first thing on screen either way, which is what the quick look's
 * flight moves (`flipFrom` finds the image).
 */
function Preview({
  part,
  hidden,
  onParts,
  ghost,
  annotated,
}: {
  part: PartDetailData
  hidden?: ReadonlySet<number>
  onParts?: (parts: number | null) => void
  ghost?: BlobHash | null
  annotated?: boolean
}) {
  const poster =
    part.thumbnail === null ? null : (
      <img
        src={part.thumbnail}
        alt={strings.parts.thumbnailAlt(part.name)}
        className="h-full w-full object-contain"
      />
    )
  if (part.tessellationL0 === null) {
    return poster === null ? null : <div className={FRAME}>{poster}</div>
  }
  if (!hasWebGL()) {
    return (
      <figure>
        {poster === null ? null : <div className={FRAME}>{poster}</div>}
        <figcaption className="mt-1 max-w-40 text-xs text-[var(--color-muted)]">
          {strings.viewer.noWebGL}
        </figcaption>
      </figure>
    )
  }
  return (
    <Suspense fallback={<div className={FRAME}>{poster}</div>}>
      {/*
        Keyed by part: the quick look and the part page both hand the same Preview a different
        part without unmounting it, and a view kept across parts keeps the last part's framing,
        its measuring tool and its picks, which would then be measured against the new part's
        entities. A new rung of the same part keeps the key, so it swaps in without a jump.
      */}
      <Viewer
        key={part.id}
        part={part}
        poster={poster}
        hidden={hidden}
        onParts={onParts}
        ghost={ghost}
        annotated={annotated}
      />
    </Suspense>
  )
}

/** The formats a slicer reads as they are, so a part in one downloads for a slicer as its own file (`lapidary-targets`). */
const slicerReads = ['stl', '3mf']

const control =
  'ease-mechanical inline-block rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px'

/**
 * A 3MF for a slicer, from a part no slicer reads as it is: asked for, written by the worker when it is not yet,
 * then offered as a download named `*.lapidary.3mf`.
 */
function SlicerExport({ part }: { part: PartDetailData }) {
  const [state, setState] = useState<
    | { kind: 'idle' }
    | { kind: 'building'; batch: BatchId | null }
    | { kind: 'ready' }
    | { kind: 'failed'; reason: string }
  >({ kind: 'idle' })
  const batch = state.kind === 'building' ? state.batch : null
  const building = useQuery({
    queryKey: ['batch', part.library, batch],
    queryFn: () => fetchBatchStatus(part.library, batch as BatchId),
    enabled: batch !== null,
    refetchInterval: (query) => (query.state.data?.finishedAt == null ? 1000 : false),
  })
  const finished = batch !== null && building.data?.finishedAt != null ? building.data : null
  useEffect(() => {
    if (finished === null) return
    const failure = finished.failed[0]
    setState(failure === undefined ? { kind: 'ready' } : { kind: 'failed', reason: failure.reason })
  }, [finished])

  const ask = () => {
    setState({ kind: 'building', batch: null })
    requestExport(part.id)
      .then((answer) =>
        setState(answer.kind === 'ready' ? { kind: 'ready' } : { kind: 'building', batch: answer.queued.batchId }),
      )
      .catch((error: unknown) => setState({ kind: 'failed', reason: error instanceof Error ? error.message : String(error) }))
  }

  if (state.kind === 'ready') {
    return (
      <a href={exportUrl(part.revision)} download className={control}>
        {strings.download.forSlicerReady}
      </a>
    )
  }
  const busy = state.kind === 'building'
  const failure = state.kind === 'failed' ? state.reason : null
  return (
    <>
      <button type="button" onClick={ask} disabled={busy} className={control}>
        {busy ? strings.download.forSlicerBuilding : strings.download.forSlicer}
      </button>
      {failure === null ? null : (
        <span role="alert" className="text-sm text-[var(--color-muted)]">
          {strings.download.forSlicerFailed(failure)}
        </span>
      )}
    </>
  )
}

export function Detail({
  part,
  actions,
  recordable = false,
  titled = false,
}: {
  part: PartDetailData
  actions?: ReactNode
  /**
   * Whether the source *form* is offered. Off by default, which is the quick-look, and on
   * for the full page — the same line `actions` draws, for the same reason: seven fields
   * inside a dialog that closes on Escape is a half-typed form somebody loses to a reflex.
   *
   * The list is shown either way, because reading a licence is the reason to look. And the
   * gallery's own controls stay in both: picking a file holds no typed state to lose, which
   * is the distinction, not "does it change the part".
   */
  recordable?: boolean
  /**
   * Whether something around this article already names the part. The quick-look's
   * `Dialog` does — its title is the element `aria-labelledby` points at — and this
   * article's own `h2` repeated the same name as a sibling heading inside the same dialog,
   * so a screen reader's heading list read the part twice. On the part's own page nothing
   * else names it, and the `h2` stays.
   */
  titled?: boolean
}) {
  // Which parts are out of the view, and how many the view drew. Both belong to one part, so a
  // choice made on one assembly never carries to the next one shown in the same place.
  const [hiddenFor, setHiddenFor] = useState({ part: part.id, hidden: NONE })
  const hidden = hiddenFor.part === part.id ? hiddenFor.hidden : NONE
  const setHidden = (next: ReadonlySet<number>) => setHiddenFor({ part: part.id, hidden: next })
  const [drawnFor, setDrawnFor] = useState<{ part: PartId; parts: number | null }>({
    part: part.id,
    parts: null,
  })
  const drawn = drawnFor.part === part.id ? drawnFor.parts : null
  const onParts = useCallback((parts: number | null) => setDrawnFor({ part: part.id, parts }), [part.id])
  // The earlier revision the comparison draws as a ghost. For this part only, as `hidden` is.
  const [ghostFor, setGhostFor] = useState<{ part: PartId; hash: BlobHash | null }>({
    part: part.id,
    hash: null,
  })
  const ghost = ghostFor.part === part.id ? ghostFor.hash : null
  const onGhost = useCallback((hash: BlobHash | null) => setGhostFor({ part: part.id, hash }), [part.id])
  // Whether the file's PMI is drawn in the view. For this part only, as `hidden` is, and off until asked.
  const [annotatedFor, setAnnotatedFor] = useState({ part: part.id, on: false })
  const annotated = annotatedFor.part === part.id && annotatedFor.on
  const setAnnotated = (on: boolean) => setAnnotatedFor({ part: part.id, on })
  // Opening in a desktop app is a checkout, which only a controlled library takes: a hobby library
  // keeps no revision for the save to come back as.
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  const controlled =
    libraries.data?.find((library) => library.id === part.library)?.mode === 'controlled' &&
    part.sourceHash !== null
  return (
    <article className="mt-4">
      <header className="mb-6 flex flex-wrap items-start gap-6">
        <Preview part={part} hidden={hidden} onParts={onParts} ghost={ghost} annotated={annotated} />
        <div>
          {titled ? null : <h2 className="text-xl font-medium">{part.name}</h2>}
          {part.partNumber === null ? null : (
            <p className="mt-1 text-sm text-[var(--color-muted)]">{part.partNumber}</p>
          )}
          <div className="mt-4 flex flex-wrap items-center gap-2">
            {/*
              Guarded, because a revision can exist with no `file` row of role `source` —
              `0007`'s recovered parts are exactly that shape. The control used to be on the
              card, which withheld it; moving it here without the guard would have offered a
              download that 404s at the route, and the card's own test is what caught it.
            */}
            {part.sourceHash === null ? (
              <span className="text-sm text-[var(--color-muted)]">
                {strings.download.noSource}
              </span>
            ) : (
              <a
                href={downloadUrl(part.revision)}
                download
                className="ease-mechanical inline-block rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
              >
                {strings.download.original}
              </a>
            )}
            {part.sourceHash !== null && part.sourceFormat !== null && !slicerReads.includes(part.sourceFormat) ? (
              <SlicerExport key={part.revision} part={part} />
            ) : null}
            {controlled ? (
              <a
                href={openLink(part.id)}
                className="ease-mechanical inline-block rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
              >
                {strings.download.openInApp}
              </a>
            ) : null}
            {/*
              A slot rather than a component, so that what changes the part stays with the
              page and the panel gets only what is safe to show in something transient.
              Removing a model from a dialog that closes on Escape is not a control this
              belongs to.
            */}
            {actions}
          </div>
          {controlled ? (
            <p className="mt-2 max-w-md text-xs text-[var(--color-muted)]">{strings.download.openInAppNote}</p>
          ) : null}
        </div>
      </header>

      <Gallery part={part.id} name={part.name} />

      <Tags part={part} recordable={recordable} />
      <Fields part={part} recordable={recordable} />

      <Sources part={part.id} recordable={recordable} />

      <Section
        title={strings.detail.geometry}
        note={
          // The key explains the mark, so it is there exactly when a mark is.
          [part.bboxMm, part.volumeMm3, part.surfaceAreaMm2].some(
            (figure) => figure?.approximate === true,
          ) ? (
            <>
              <span aria-hidden="true" className="mr-1">
                {strings.detail.approximate}
              </span>
              <span>{strings.detail.approximateKey}</span>
            </>
          ) : undefined
        }
      >
        <Row label={strings.detail.triangles}>
          {part.triangleCount === null
            ? strings.detail.unknown
            : strings.detail.trianglesValue(part.triangleCount)}
        </Row>
        <Row label={strings.detail.boundingBox}>
          {part.bboxMm === null ? (
            strings.detail.unknown
          ) : (
            <Figure
              figure={part.bboxMm}
              render={(mm) => strings.detail.boundingBoxValue(mm)}
            />
          )}
        </Row>
        <Row label={strings.detail.volume}>
          {/*
            An open mesh has no volume, and the reason is worth a sentence rather than a
            blank a reader would take for zero. `isWatertight === false` is the reason;
            `null` with a closed mesh is a figure nobody recorded, which is a different
            fact and gets the other word.
          */}
          {part.volumeMm3 === null ? (
            part.isWatertight === false ? (
              strings.detail.volumeUnavailable
            ) : (
              strings.detail.unknown
            )
          ) : (
            <Figure figure={part.volumeMm3} render={strings.detail.volumeValue} />
          )}
        </Row>
        <Row label={strings.detail.surfaceArea}>
          {part.surfaceAreaMm2 === null ? (
            strings.detail.unknown
          ) : (
            <Figure figure={part.surfaceAreaMm2} render={strings.detail.surfaceAreaValue} />
          )}
        </Row>
        <Row label={strings.detail.watertight}>
          {part.isWatertight === null
            ? strings.detail.unknown
            : part.isWatertight
              ? strings.detail.watertightYes
              : strings.detail.watertightNo}
        </Row>
      </Section>

      {part.structure === null ? null : (
        <Assembly hash={part.structure} hidden={hidden} onHide={setHidden} drawn={drawn} />
      )}

      <Specified part={part} annotated={annotated} onAnnotate={setAnnotated} />

      <Section title={strings.detail.file}>
        <Row label={strings.detail.format}>
          {part.sourceFormat === null ? strings.detail.unknown : part.sourceFormat}
        </Row>
        <Row label={strings.detail.size}>
          {/*
            The same three-way choice the card makes, and it must stay the same: a
            compressed part whose ingested size did not arrive cannot be called
            uncompressed, so `storedRaw` there is not the weaker claim but the wrong one.
          */}
          {part.storedBytes === null
            ? strings.detail.unknown
            : part.compressed !== true
              ? strings.parts.storedRaw(part.storedBytes)
              : part.sourceBytes !== null
                ? strings.parts.storedCompressed(part.storedBytes, part.sourceBytes)
                : strings.parts.storedSize(part.storedBytes)}
        </Row>
        <Row label={strings.detail.sourceHash}>
          {part.sourceHash === null ? (
            strings.detail.unknown
          ) : (
            // `break-all`: sixty-four hex digits have no break opportunity, and in the quick look
            // they ran out past the panel edge.
            <code className="text-xs break-all">{part.sourceHash}</code>
          )}
        </Row>
        <Row label={strings.detail.preview3d}>
          {/*
            The first caller `GET /api/blob/{blake3}` has ever had. Every ingest since the
            LOD ladder wrote this rung and nothing carried its hash, so the bytes were
            written, reference-counted and unreachable. A link, not a fetch, for the reason
            the download link is one: the browser does the transfer.
          */}
          {part.tessellationL0 === null ? (
            strings.detail.noPreview3d
          ) : (
            <a
              href={blobUrl(part.tessellationL0)}
              download
              className="underline underline-offset-2"
            >
              {part.tessellationL0Bytes === null
                ? strings.detail.download3d
                : strings.detail.download3dSized(part.tessellationL0Bytes)}
            </a>
          )}
        </Row>
      </Section>

      <Section title={strings.detail.identity}>
        <Row label={strings.detail.sourcePath}>
          <code className="text-xs">{part.sourcePath}</code>
        </Row>
        <Row label={strings.detail.revision}>{part.revLabel}</Row>
        {part.lock === null ? null : (
          <Row label={strings.detail.checkedOut}>
            <LockLine part={part.id} lock={part.lock} recordable={recordable} />
          </Row>
        )}
        <Row label={strings.detail.kernel}>
          {part.kernelVersion === null ? strings.detail.unknown : part.kernelVersion}
        </Row>
      </Section>

      <History part={part.id} onGhost={onGhost} />
    </article>
  )
}

/**
 * The first of the three steps, and the only one reachable from a part's own page.
 *
 * On success this navigates to the grid rather than staying: every read path filters
 * `deleted_at`, so this route would 404 on the next render and show "could not open this
 * part" for a part the user just successfully removed. Leaving is the honest response to a
 * page that no longer describes anything.
 *
 * No confirmation dialog. The action is reversible indefinitely, changes nothing on disk,
 * and `strings.removal.removeHint` says so next to the button before it is pressed —
 * confirming it would teach people to click through the dialog that purge actually needs.
 */

/**
 * The assembly tree a CAD kernel read, for a STEP or IGES part.
 *
 * Nested native disclosures rather than a hand-rolled ARIA tree: a `<summary>` takes focus and
 * opens on Enter or Space with no script, and assistive technology announces it expanded or
 * collapsed. The top level starts open, since it is the assembly itself; everything under it
 * starts closed. A single part with nothing under it is not a tree worth a section.
 */
/**
 * What the file specifies about the part's sizes and form: its dimensions, geometric tolerances and
 * datums. Said as specified, never through `Figure`: a designer's value is neither a measurement
 * nor approximate, and the ≈ mark would claim it was one or the other. Each annotation says which
 * face it is on by the entity measurement reads there, when the part has entities.
 */
function Specified({
  part,
  annotated,
  onAnnotate,
}: {
  part: PartDetailData
  annotated: boolean
  onAnnotate: (on: boolean) => void
}) {
  const pmi = useQuery({
    queryKey: ['pmi', part.pmi],
    queryFn: () => fetchPmi(part.pmi as BlobHash),
    enabled: part.pmi !== null,
  })
  const entities = useQuery({
    queryKey: ['entities', part.entities],
    queryFn: () => fetchEntities(part.entities as BlobHash),
    enabled: part.pmi !== null && part.entities !== null,
  })
  if (part.pmi === null) return null
  const heading = (
    <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
      {strings.pmi.title}
    </h3>
  )
  if (pmi.isError) {
    return (
      <section className="mb-6">
        {heading}
        <p role="alert" className="max-w-prose text-sm text-[var(--color-muted)]">
          {strings.pmi.failed}
        </p>
      </section>
    )
  }
  if (pmi.data === undefined) return null
  const where = (faces: readonly PmiFace[]) =>
    faces
      .map((face) => {
        const entity = (entities.data ?? []).find(
          (candidate) =>
            'face' in candidate && candidate.prototype === face.prototype && candidate.face === face.face,
        )
        return strings.pmi.face(face.face === null ? null : (entity?.type ?? ''))
      })
      .join(', ')
  const annotations = annotationsOf(pmi.data)
  // Which annotations the view can place: those on a face measurement reads as an entity. Asked of the
  // entities unplaced, since which faces exist does not depend on where they were drawn. Nothing is said
  // until they arrive, and faces that could not be read are said once below rather than blamed on each.
  const { undrawn } = entities.data === undefined ? { undrawn: new Set<number>() } : labelsFor(annotations, entities.data)
  // A view to draw in: a rung, WebGL, and entities to say where each face is.
  const drawable = part.tessellationL0 !== null && part.entities !== null && hasWebGL()
  const rows = annotations.map((annotation, index) => ({
    what: annotation.text,
    on: where(annotation.faces),
    undrawn: undrawn.has(index) ? strings.pmi.notDrawn(annotation.faces.every((face) => face.face === null)) : null,
  }))
  return (
    <section className="mb-6">
      {heading}
      <p className="mb-2 flex flex-wrap items-center gap-2 text-xs text-[var(--color-muted)]">
        {strings.pmi.note}
        {drawable ? (
          <button type="button" aria-pressed={annotated} onClick={() => onAnnotate(!annotated)} className={CONTROL}>
            {strings.pmi.showInView}
          </button>
        ) : null}
      </p>
      {annotated && drawable && entities.isError ? (
        <p role="alert" className="mb-2 max-w-prose text-xs text-[var(--color-muted)]">
          {strings.pmi.facesUnread}
        </p>
      ) : null}
      <ul role="list" className="space-y-0.5 text-sm">
        {rows.map((row, index) => (
          <li key={index}>
            {row.what}
            {row.on === '' ? null : (
              <span className="text-[var(--color-muted)]">
                {' · '}
                {row.on}
              </span>
            )}
            {annotated && drawable && row.undrawn !== null ? (
              <span className="text-[var(--color-muted)]">
                {' · '}
                {row.undrawn}
              </span>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  )
}

const NONE: ReadonlySet<number> = new Set()

const CONTROL =
  'rounded-sm px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-bright)]'

/** What the tree may do to the view: which parts are out of it, how many there are, and the setter. */
type Visibility = {
  hidden: ReadonlySet<number>
  parts: number
  onHide: (hidden: ReadonlySet<number>) => void
}

/** How many placed parts are under a node, counting itself when it is one. */
function leaves(node: AssemblyNode): number {
  return node.children.length === 0 ? 1 : node.children.reduce((sum, child) => sum + leaves(child), 0)
}

/** Each node's first placed part, in the depth-first order the view counts triangles in. */
function firstLeaves(nodes: readonly AssemblyNode[], from = 0): number[] {
  let at = from
  return nodes.map((node) => {
    const first = at
    at += leaves(node)
    return first
  })
}

/**
 * The assembly tree, and where the view drew the same parts, a way to hide them. `drawn` is how
 * many placed parts the view's rung counts; the buttons appear only when that is this tree's
 * count. A mesh, a rung from before parts were counted, or a browser that draws no 3D has nothing
 * to hide a part in.
 */
function Assembly({
  hash,
  hidden,
  onHide,
  drawn,
}: {
  hash: BlobHash
  hidden: ReadonlySet<number>
  onHide: (hidden: ReadonlySet<number>) => void
  drawn: number | null
}) {
  const tree = useQuery({ queryKey: ['structure', hash], queryFn: () => fetchStructure(hash) })
  const heading = (
    <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
      {strings.detail.assembly}
    </h3>
  )
  if (tree.isError) {
    return (
      <section className="mb-6">
        {heading}
        <p role="alert" className="max-w-prose text-sm text-[var(--color-muted)]">
          {strings.detail.assemblyFailed}
        </p>
      </section>
    )
  }
  if (tree.data === undefined) return null
  const { roots, parts, prototypes } = tree.data
  if (roots.length === 1 && roots[0]?.children.length === 0) return null
  const visibility = drawn === parts ? { hidden, parts, onHide } : null
  const firsts = firstLeaves(roots)
  // ponytail: every node is in the DOM, open or not. Render a branch's children only once it
  // is opened if a 10,000-part assembly makes this page slow.
  return (
    <section className="mb-6">
      {heading}
      <p className="mb-2 flex items-center gap-2 text-xs text-[var(--color-muted)]">
        {strings.detail.assemblyCounts(parts, prototypes)}
        {visibility === null || hidden.size === 0 ? null : (
          <button type="button" onClick={() => onHide(NONE)} className={CONTROL}>
            {strings.detail.showAll}
          </button>
        )}
      </p>
      <ul role="list" className="text-sm">
        {roots.map((node, index) => (
          <AssemblyBranch
            key={index}
            node={node}
            first={firsts[index] ?? 0}
            visibility={visibility}
            open
          />
        ))}
      </ul>
    </section>
  )
}

function AssemblyBranch({
  node,
  first,
  visibility,
  open = false,
}: {
  node: AssemblyNode
  first: number
  visibility: Visibility | null
  open?: boolean
}) {
  const count = leaves(node)
  const mine = (part: number) => part >= first && part < first + count
  const allHidden =
    visibility !== null &&
    Array.from({ length: count }, (_, offset) => first + offset).every((part) =>
      visibility.hidden.has(part),
    )
  const controls =
    visibility === null ? null : (
      <span className="ml-2 inline-flex gap-1">
        <button
          type="button"
          aria-label={allHidden ? strings.detail.showPart(node.name) : strings.detail.hidePart(node.name)}
          onClick={(event) => {
            // Inside a `summary` a click would also open or close the branch.
            event.preventDefault()
            const next = new Set(visibility.hidden)
            for (let part = first; part < first + count; part++) {
              if (allHidden) next.delete(part)
              else next.add(part)
            }
            visibility.onHide(next)
          }}
          className={CONTROL}
        >
          {allHidden ? strings.detail.show : strings.detail.hide}
        </button>
        <button
          type="button"
          aria-label={strings.detail.isolatePart(node.name)}
          onClick={(event) => {
            event.preventDefault()
            const others = Array.from({ length: visibility.parts }, (_, part) => part).filter(
              (part) => !mine(part),
            )
            visibility.onHide(new Set(others))
          }}
          className={CONTROL}
        >
          {strings.detail.isolate}
        </button>
      </span>
    )
  // A part out of the view is named in the muted colour, so the tree shows what is hidden.
  const muted = allHidden ? ' text-[var(--color-muted)]' : ''
  if (node.children.length === 0) {
    return (
      <li className={`py-0.5 pl-4${muted}`}>
        {node.name}
        {controls}
      </li>
    )
  }
  const firsts = firstLeaves(node.children, first)
  return (
    <li>
      <details open={open}>
        <summary className={`cursor-pointer py-0.5${muted}`}>
          {node.name}
          {controls}
        </summary>
        <ul role="list" className="ml-1.5 border-l border-[var(--color-edge)] pl-2">
          {node.children.map((child, index) => (
            <AssemblyBranch
              key={index}
              node={child}
              first={firsts[index] ?? first}
              visibility={visibility}
            />
          ))}
        </ul>
      </details>
    </li>
  )
}

/**
 * Every revision of the part, newest first (Phase 4 slice 1). Drawn once there is more than
 * one: a history of one revision is the Identity row above it, said twice.
 *
 * Rows and inline thumbnails only, so the open path. Each volume goes through `Figure`, so a
 * mesh-derived figure keeps its ≈ here as everywhere, and each revision's original is a plain
 * download link, byte-identical to what that revision ingested.
 */
function History({ part, onGhost }: { part: PartId; onGhost: (hash: BlobHash | null) => void }) {
  const revisions = useQuery({
    queryKey: ['revisions', part],
    queryFn: () => fetchRevisions(part),
  })
  if (revisions.isError) {
    return (
      <p role="alert" className="mb-6 text-sm text-[var(--color-muted)]">
        {strings.detail.historyFailed}
      </p>
    )
  }
  const all = revisions.data ?? []
  if (all.length < 2) return null
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {strings.detail.history}
      </h3>
      <ol role="list" className="flex flex-col gap-2 text-sm">
        {all.map((revision) => (
          <li key={revision.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
            {revision.thumbnail === null ? null : (
              <img
                src={revision.thumbnail}
                alt=""
                className="size-10 rounded-[var(--radius-ctl)] object-cover"
              />
            )}
            <span className="font-medium">{strings.detail.historyRevision(revision.revLabel)}</span>
            <span className="text-[var(--color-muted)]">
              {strings.detail.origin[revision.origin]} ·{' '}
              {strings.detail.historyDate(revision.createdAt)}
            </span>
            {revision.volumeMm3 === null ? null : (
              <Figure figure={revision.volumeMm3} render={strings.detail.volumeValue} />
            )}
            {revision.deltaFromParent?.volumeMm3 ? (
              <Change
                delta={revision.deltaFromParent.volumeMm3}
                render={strings.detail.volumeChange}
              />
            ) : null}
            <a
              href={downloadUrl(revision.id)}
              download
              className="underline underline-offset-2"
            >
              {strings.download.original}
            </a>
          </li>
        ))}
      </ol>
      {/* Keyed: its picks are revision ids of this part, and another part's would be refused. */}
      <Compare key={part} part={part} revisions={all} onGhost={onGhost} />
    </section>
  )
}

/**
 * Who holds the part's check-out, and since when. Releasing it is offered only where the page
 * is recordable, and behind a dialog, because the holder's next save will be refused.
 */
function LockLine({
  part,
  lock,
  recordable,
}: {
  part: PartId
  lock: PartLock
  recordable: boolean
}) {
  const queryClient = useQueryClient()
  const [asking, setAsking] = useState(false)
  const release = useMutation({
    mutationFn: () => releaseLock(part),
    onSuccess: () => {
      setAsking(false)
      void queryClient.invalidateQueries({ queryKey: ['part', part] })
    },
  })
  return (
    <span className="flex flex-wrap items-center gap-2">
      <span>{strings.detail.checkedOutBy(lock.holder, lock.takenAt)}</span>
      {!recordable ? null : (
        <button
          type="button"
          onClick={() => setAsking(true)}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-0.5 text-xs duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.detail.releaseLock}
        </button>
      )}
      {!asking ? null : (
        <Dialog title={strings.detail.releaseLockTitle} onClose={() => setAsking(false)}>
          <p className="mt-3 text-sm">{strings.detail.releaseLockBody(lock.holder)}</p>
          {release.isError ? (
            <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
              {strings.detail.releaseLockFailed}
            </p>
          ) : null}
          <div className="mt-4 flex justify-end gap-2">
            <button
              type="button"
              autoFocus
              onClick={() => setAsking(false)}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
            >
              {strings.folders.cancel}
            </button>
            <button
              type="button"
              disabled={release.isPending}
              onClick={() => release.mutate()}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
            >
              {strings.detail.releaseLockConfirm}
            </button>
          </div>
        </Dialog>
      )}
    </span>
  )
}

/** A change between two revisions, through `Figure`, so a difference of mesh figures keeps its ≈. */
function Change({
  delta,
  render,
}: {
  delta: Delta
  render: (change: number, percent: number | null) => string
}) {
  return (
    <Figure
      figure={{ value: delta.change, approximate: delta.approximate }}
      render={(value) => render(value, delta.percent)}
    />
  )
}

/**
 * Any two revisions, figure by figure. Opens on the newest against the one before it, the change
 * a person most likely came for, and asks the server, which keeps the ≈ rule in one place
 * instead of a second copy of the arithmetic here.
 */
function Compare({
  part,
  revisions,
  onGhost,
}: {
  part: PartId
  revisions: PartRevision[]
  onGhost: (hash: BlobHash | null) => void
}) {
  const [from, setFrom] = useState<RevisionId | undefined>(revisions[1]?.id)
  const [to, setTo] = useState<RevisionId | undefined>(revisions[0]?.id)
  const [ghosted, setGhosted] = useState(false)
  // From's own mesh: the L1 somebody opened while it was current, else the L0 ingest wrote. Handed
  // to the 3D view only while the box is ticked, and taken back when this comparison goes away.
  const earlier = revisions.find((revision) => revision.id === from)
  const ghost = earlier?.tessellationL1 ?? earlier?.tessellationL0 ?? null
  useEffect(() => {
    onGhost(ghosted ? ghost : null)
  }, [ghosted, ghost, onGhost])
  useEffect(() => () => onGhost(null), [onGhost])
  // A coarse outline beside a finer part must not read as a change of shape.
  const coarse = earlier?.tessellationL1 == null && revisions[0]?.tessellationL1 != null
  const compared = useQuery({
    queryKey: ['diff', part, from, to],
    queryFn: () => fetchDiff(part, from as RevisionId, to as RevisionId),
    enabled: from !== undefined && to !== undefined,
  })
  const pick = (
    label: string,
    value: RevisionId | undefined,
    onChange: (id: RevisionId) => void,
  ) => (
    <label className="flex items-center gap-2">
      {label}
      <select
        value={value}
        onChange={(event) => onChange(event.target.value as RevisionId)}
        className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
      >
        {revisions.map((revision) => (
          <option key={revision.id} value={revision.id}>
            {strings.detail.historyRevision(revision.revLabel)}
          </option>
        ))}
      </select>
    </label>
  )
  const diff = compared.data
  const rows =
    diff === undefined
      ? []
      : [
          { label: strings.detail.volume, delta: diff.volumeMm3, render: strings.detail.volumeChange },
          {
            label: strings.detail.surfaceArea,
            delta: diff.surfaceAreaMm2,
            render: strings.detail.areaChange,
          },
          ...([0, 1, 2] as const).map((axis) => ({
            label: strings.detail.boundingBoxAxis(axis),
            delta: diff.bboxMm?.[axis] ?? null,
            render: strings.detail.lengthChange,
          })),
          {
            label: strings.detail.triangles,
            delta: diff.triangleCount,
            render: strings.detail.countChange,
          },
          { label: strings.detail.faces, delta: diff.faceCount, render: strings.detail.countChange },
          { label: strings.detail.edges, delta: diff.edgeCount, render: strings.detail.countChange },
        ]
  return (
    <div className="mt-3 text-sm">
      <div className="mb-2 flex flex-wrap items-center gap-3 text-xs text-[var(--color-muted)]">
        {strings.detail.compare}
        {pick(strings.detail.compareFrom, from, setFrom)}
        {pick(strings.detail.compareTo, to, setTo)}
        {earlier === undefined ? null : ghost === null ? (
          <span>{strings.detail.ghostNoMesh(earlier.revLabel)}</span>
        ) : (
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={ghosted}
              onChange={(event) => setGhosted(event.target.checked)}
            />
            {strings.detail.ghost}
          </label>
        )}
      </div>
      {ghosted && ghost !== null && coarse ? (
        <p className="mb-2 text-xs text-[var(--color-muted)]">{strings.detail.ghostCoarse}</p>
      ) : null}
      {compared.isError ? (
        <p role="alert" className="text-[var(--color-muted)]">
          {strings.detail.compareFailed}
        </p>
      ) : null}
      {diff === undefined ? null : (
        <table>
          <thead>
            <tr>
              <th scope="col" className="pr-6 text-left font-normal text-[var(--color-muted)]">
                {strings.detail.compareFigure}
              </th>
              <th scope="col" className="text-left font-normal text-[var(--color-muted)]">
                {strings.detail.compareChange}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.label}>
                <th scope="row" className="pr-6 text-left font-normal text-[var(--color-muted)]">
                  {row.label}
                </th>
                <td>
                  {row.delta ? (
                    <Change delta={row.delta} render={row.render} />
                  ) : (
                    strings.detail.notInBoth
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

function Section({
  title,
  note,
  children,
}: {
  title: string
  note?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {title}
      </h3>
      {note === undefined ? null : (
        <p className="mb-2 text-xs text-[var(--color-muted)]">{note}</p>
      )}
      <dl className="grid grid-cols-[minmax(8rem,max-content)_1fr] gap-x-6 gap-y-1 text-sm">
        {children}
      </dl>
    </section>
  )
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-[var(--color-muted)]">{label}</dt>
      <dd>{children}</dd>
    </>
  )
}

/**
 * The pictures somebody attached, and the control that attaches one.
 *
 * **Above the render rather than instead of it.** The generated view is the honest picture
 * of the geometry and stays; a photograph is what the geometry cannot show — the finish, the
 * colour, the thing next to a hand. `part_image`'s ordering column is what lets both exist,
 * which was an owner decision and not an accident of schema.
 *
 * Rendered inside `Detail`, so it appears on the page and in the grid's quick-look from one
 * definition. That is the same reason `Detail` is shared at all.
 */
function Gallery({ part, name }: { part: PartId; name: string }) {
  const queryClient = useQueryClient()
  const picker = useRef<HTMLInputElement>(null)
  /** The server's own sentence about a refused file — it names the limit that was broken. */
  const [refusal, setRefusal] = useState<string | null>(null)
  /** What the last accepted upload was stored at, so a silent resize is not silent. */
  const [stored, setStored] = useState<{ width: number; height: number } | null>(null)
  /** The address being pasted, when one is. `null` means the field is not open. */
  const [url, setUrl] = useState<string | null>(null)

  const images = useQuery({
    queryKey: ['part-images', part],
    queryFn: () => fetchPartImages(part),
  })

  // One mutation for both ways in, because the server answers both the same way: a stored
  // size, or a sentence saying what was wrong with what it was given. A second mutation
  // would be a second copy of the refusal handling below.
  const add = useMutation({
    mutationFn: (source: File | string) => uploadPartImage(part, source),
    onMutate: () => {
      setRefusal(null)
      setStored(null)
    },
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setRefusal(result.message)
        return
      }
      setStored({ width: result.stored.width, height: result.stored.height })
      setUrl(null)
      void queryClient.invalidateQueries({ queryKey: ['part-images', part] })
    },
  })

  const gallery = images.data ?? []
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.images.title}
      </h3>
      {/* Same reason as `Sources`: an empty gallery and one we could not read are different. */}
      {images.isError ? (
        <p role="alert" className="mb-2 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.images.galleryFailed}
        </p>
      ) : null}
      {gallery.length === 0 ? null : (
        <ul role="list" className="mb-2 flex list-none flex-wrap gap-2">
          {gallery.map((image, index) => (
            <li key={image.id}>
              <Framed part={part} image={image} label={strings.images.alt(name, index)} />
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap items-center gap-2">
        {/*
          Hidden, and driven by the button beside it: a bare file input is unstyleable across
          browsers and announces itself as "Choose file", which is not what this does. The
          button carries the label and the input carries the capability.
        */}
        <input
          ref={picker}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0]
            if (file !== undefined) add.mutate(file)
            // Cleared so that picking the same file twice in a row fires `change` the second
            // time: without this, a failed upload could not be retried with the same file.
            event.target.value = ''
          }}
        />
        <button
          type="button"
          onClick={() => picker.current?.click()}
          disabled={add.isPending}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {add.isPending ? strings.images.adding : strings.images.add}
        </button>
        {url !== null ? null : (
          <button
            type="button"
            onClick={() => {
              setRefusal(null)
              setUrl('')
            }}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.images.addFromUrl}
          </button>
        )}
      </div>
      {/*
        A real `<form>`, so Enter submits it — pasting an address and pressing Enter is what
        a person will do, and an input with a button beside it silently does nothing. The
        browser's own `type="url"` validity check is deliberately not leaned on: the refusals
        that matter are about where the address points, and only the server can know those.
      */}
      {url === null ? null : (
        <form
          className="mt-2 flex flex-wrap items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            const trimmed = url.trim()
            if (trimmed !== '') add.mutate(trimmed)
          }}
        >
          <input
            type="url"
            value={url}
            autoFocus
            onChange={(event) => setUrl(event.target.value)}
            placeholder={strings.images.urlPlaceholder}
            aria-label={strings.images.urlLabel}
            className="min-w-64 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-1 text-xs"
          />
          <button
            type="submit"
            disabled={add.isPending || url.trim() === ''}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {add.isPending ? strings.images.fetching : strings.images.fetch}
          </button>
          <button
            type="button"
            onClick={() => setUrl(null)}
            className="rounded px-2 py-1 text-xs text-[var(--color-muted)]"
          >
            {strings.images.cancelUrl}
          </button>
        </form>
      )}
      {refusal === null ? null : (
        <p role="alert" className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {refusal}
        </p>
      )}
      {stored === null ? null : (
        <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {strings.images.resized(stored.width, stored.height)}
        </p>
      )}
      {!add.isError ? null : (
        <p role="alert" className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {strings.images.failed}
        </p>
      )}
    </section>
  )
}
