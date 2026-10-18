import type { FileRoot } from '../bindings/FileRoot'
import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { Fragment, Suspense, lazy, useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import {
  addPartSource,
  blobUrl,
  DEFAULT_LIBRARY_ID,
  downloadUrl,
  fileLink,
  exportUrl,
  fetchBatchStatus,
  fetchFields,
  fetchLibraries,
  fetchPartImages,
  fetchPartSources,
  describePart,
  fetchRevisions,
  requestExport,
  setFieldValue,
  reorderPartImages,
  setImageFraming,
  setPartMaterials,
  setPartTags,
  unsetPartMaterials,
  uploadPartImage,
} from '../lib/api'
import { strings } from '../lib/strings'
import { Dialog } from './Dialog'
import { Icon } from './Icon'
import { Figure } from './Figure'
import { hasWebGL } from '../lib/viewer-math'
import { arrive } from '../lib/motion'
import { breakable, pictureStyle } from './Card'
import Lattice from './Lattice'
import { Crossfade, Presence } from '../lib/motion-react'
import type {
  BatchId,
  BlobHash,
  CustomField,
  PackContents,
  PartDetail as PartDetailData,
  PartId,
  PartImage,
  PartImageId,
  PartRevision,
  RevisionId,
} from '../lib/types'


// Hoisted, for the reason `ShowInFolder` narrows where it does: a literal compared inside JSX reads to
// the bare-strings gate as a label.
const LIBRARY: FileRoot = 'library'
const INBOX: FileRoot = 'inbox'
/**
 * A part's pictures changed: its own gallery, and every grid page, whose cards show its first picture.
 * The grid was left out once, and a picture added on the part's page did not reach its card until a
 * reload (M1, the owner's first look).
 */
function picturesChanged(queryClient: QueryClient, part: PartId) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: ['part-images', part] }),
    queryClient.invalidateQueries({ queryKey: ['parts'] }),
  ])
}

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
    onSuccess: () => picturesChanged(queryClient, part),
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
    // The chosen picture as a card crops it: the strip's tiles choose, so the crop is set here.
    <div className="mt-3 flex items-start gap-3">
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
        className="block size-16 flex-none cursor-crosshair overflow-hidden rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)]"
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
      <div className="flex min-w-0 flex-col items-start gap-1.5 text-xs">
        <p className="text-[var(--color-muted)]">{strings.images.framing}</p>
        {/*
          Two states, so a toggle rather than a pair of radios. `cover` fills the tile and
          loses the edges; `contain` shows the whole picture and letterboxes it.
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
          className="ease-mechanical inline-flex h-6 items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px hover:text-[var(--color-text)] disabled:opacity-50"
        >
          {filling ? strings.images.fitWhole : strings.images.fitFill}
        </button>
        {reframe.isError ? (
          <p role="alert" className="max-w-prose leading-snug text-[var(--color-muted)]">
            {strings.images.reframeFailed}
          </p>
        ) : null}
      </div>
    </div>
  )
}

/** What a model's page says about it (A15): four lines, and the rest when asked. */
function Description({ text }: { text: string }) {
  const [whole, setWhole] = useState(false)
  const long = text.length > 320 || text.split('\n').length > 4
  return (
    <div className="mt-2 max-w-prose text-xs leading-relaxed text-[var(--color-text)]">
      <p className={whole || !long ? 'whitespace-pre-line' : 'line-clamp-4 whitespace-pre-line'}>{text}</p>
      {!long ? null : (
        <button
          type="button"
          aria-expanded={whole}
          onClick={() => setWhole(!whole)}
          className="mt-1 text-[var(--color-muted)] underline"
        >
          {whole ? strings.sources.showLess : strings.sources.showAll}
        </button>
      )}
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
  /** The fetch form (A2), also closed until asked for. */
  const [fetching, setFetching] = useState(false)
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
    <section className="mb-8">
      <Label
        title={strings.sources.title}
        action={
          recordable && !open ? (
            <Opener
              name={strings.sources.add}
              onOpen={() => {
                setRefusal(null)
                setOpen(true)
              }}
            />
          ) : undefined
        }
      />
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
              className="rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm"
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
              {source.description ? <Description text={source.description} /> : null}
            </li>
          ))}
        </ul>
      )}
      {!recordable || !open ? null : (
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
                className="mt-1 block h-8 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 text-sm text-[var(--color-text)] focus:border-[var(--color-accent)]"
              />
            </label>
          ))}
          <div className="flex items-center gap-2">
            <button
              type="submit"
              disabled={add.isPending}
              className={QUIET}
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
      )}
      {refusal === null ? null : (
        <p role="alert" className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {refusal}
        </p>
      )}
      {!recordable ? null : fetching ? (
        <FetchDetails part={part} onClose={() => setFetching(false)} />
      ) : (
        <button type="button" onClick={() => setFetching(true)} className={`${QUIET} mt-3`}>
          {strings.sources.fetch}
        </button>
      )}
    </section>
  )
}

/**
 * "Fetch details" (A2): the model's page fills in the part — source, licence, tags, pictures — and
 * nothing is downloaded. It runs as a job, so the page asks again a moment later rather than waiting.
 */
function FetchDetails({ part, onClose }: { part: PartId; onClose: () => void }) {
  const queryClient = useQueryClient()
  const [url, setUrl] = useState('')
  const [note, setNote] = useState<string | null>(null)
  const fetchNow = useMutation({
    mutationFn: () => describePart(part, url.trim()),
    onSuccess: () => {
      setNote(strings.sources.fetching)
      setUrl('')
      // ponytail: two timed re-reads rather than watching the batch; the job is one page and its pictures.
      for (const wait of [2500, 8000]) {
        setTimeout(() => {
          void queryClient.invalidateQueries({ queryKey: ['part-sources', part] })
          picturesChanged(queryClient, part)
          void queryClient.invalidateQueries({ queryKey: ['part', part] })
        }, wait)
      }
    },
    onError: (error) => setNote(error.message),
  })
  return (
    <form
      className="mt-3 flex max-w-lg flex-wrap items-center gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (url.trim() !== '' && !fetchNow.isPending) fetchNow.mutate()
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') onClose()
      }}
    >
      <input
        type="url"
        inputMode="url"
        autoFocus
        value={url}
        onChange={(event) => setUrl(event.target.value)}
        aria-label={strings.sources.fetchLabel}
        placeholder={strings.sources.fetchPlaceholder}
        className="h-8 min-w-0 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 text-sm text-[var(--color-text)]"
      />
      <button type="submit" disabled={url.trim() === '' || fetchNow.isPending} className={QUIET_FIELD}>
        {strings.sources.fetchNow}
      </button>
      <button
        type="button"
        onClick={onClose}
        className="h-8 rounded px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]"
      >
        {strings.detail.cancel}
      </button>
      {note === null ? null : (
        <p role="status" className="w-full text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </form>
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
 * One tag, as a link to its page: the same grid narrowed to it, with the tags most often beside it.
 *
 * The library rides along, left off only for the default one exactly as `AppFrame`'s places leave it
 * off — without it a tag on a model in the second library would open the first library's tag page,
 * which is the same word about a different set of models.
 *
 * No `aria-label`. The accessible name is the visible text, which is the tag, so WCAG 2.5.3 holds by
 * construction rather than by our remembering to repeat the word.
 */
function TagLink({ library, tag }: { library: PartDetailData['library']; tag: string }) {
  return (
    <Link
      to="/tags/$tag"
      params={{ tag }}
      search={library === DEFAULT_LIBRARY_ID ? {} : { library }}
      className="ease-mechanical rounded-sm duration-[var(--duration-fast)] hover:text-[var(--color-bright)]"
    >
      {tag}
    </Link>
  )
}

/**
 * A list of words a person gives the part: its tags, or its materials. Listed wherever the part is
 * shown, and edited only where `recordable` is on, for the reason `Detail` gives: a tag half-typed
 * into a dialog that closes on Escape is a tag somebody loses.
 *
 * Every change sends the whole list, which is what the route takes, and the part is read again
 * afterwards, so the page shows what the server kept rather than what was typed.
 */
function WordList({
  part,
  recordable,
  values,
  text,
  save: send,
  note = null,
  reset,
  chip,
}: {
  part: PartDetailData
  recordable: boolean
  values: readonly string[]
  text: { title: string; field: string; add: string; open: string; saving: string; remove: (value: string) => string }
  save: (part: PartDetailData['id'], values: readonly string[]) => Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }>
  /** A line under the list, such as where its values came from. */
  note?: string | null
  /**
   * Materials only: hand the part back to what its file states. Offered exactly while there is
   * something to undo — a typed list, empty or not — and absent for tags, which no file ever states.
   */
  reset?: (part: PartDetailData['id']) => Promise<{ kind: 'saved' } | { kind: 'refused'; message: string }>
  /**
   * How one value is drawn, when the value is a place you can go. Tags have a page of their own —
   * every model in this library carrying it, and the tags usually beside it — and materials do not, so
   * this is a slot rather than a flag: nothing here knows what a tag is.
   */
  chip?: (value: string) => ReactNode
}) {
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState('')
  const [refusal, setRefusal] = useState<string | null>(null)
  /** The add-form stands closed until asked for (DESIGN.md § The stage and filmstrip, the column beside it). */
  const [adding, setAdding] = useState(false)
  const settle = (result: { kind: 'saved' } | { kind: 'refused'; message: string }) => {
    if (result.kind === 'refused') {
      setRefusal(result.message)
      return
    }
    setDraft('')
    void queryClient.invalidateQueries({ queryKey: ['part', part.id] })
    // A part's mass is worked out from its materials when read, so its history and comparison read again.
    void queryClient.invalidateQueries({ queryKey: ['revisions', part.id] })
    void queryClient.invalidateQueries({ queryKey: ['diff', part.id] })
  }
  const save = useMutation({
    mutationFn: (next: readonly string[]) => send(part.id, next),
    onMutate: () => setRefusal(null),
    onSuccess: settle,
  })
  const hand = useMutation({
    mutationFn: () => (reset as NonNullable<typeof reset>)(part.id),
    onMutate: () => setRefusal(null),
    onSuccess: settle,
  })
  const tags = values
  // A note is a fact about this list — "holds no material" — so an empty list that has one is still
  // worth a section, read-only or not. Without it the one state the whole item exists for would show
  // as nothing at all wherever the list cannot be edited.
  if (tags.length === 0 && !recordable && note === null) return null
  return (
    <section className="mb-8">
      <Label
        title={text.title}
        action={recordable && !adding ? <Opener name={text.open} onOpen={() => setAdding(true)} /> : undefined}
      />
      {tags.length === 0 ? null : (
        <ul role="list" className="mb-2 flex list-none flex-wrap gap-1">
          {tags.map((tag) => (
            <li
              key={tag}
              className="flex h-6 items-center gap-1.5 rounded-[4px] border border-[var(--color-border)] bg-[var(--color-raised)] px-2 text-xs text-[var(--color-text)]"
            >
              {chip === undefined ? tag : chip(tag)}
              {recordable ? (
                <button
                  type="button"
                  aria-label={text.remove(tag)}
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
      {recordable && adding ? (
        // Stays open after a save, so several can be added in a row; Escape or Cancel closes it.
        <form
          className="flex max-w-sm items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (draft.trim() !== '') save.mutate([...tags, draft])
          }}
          onKeyDown={(event) => {
            if (event.key === 'Escape') setAdding(false)
          }}
        >
          <label className="block flex-1 text-xs text-[var(--color-muted)]">
            {text.field}
            <input
              value={draft}
              autoFocus
              onChange={(event) => setDraft(event.target.value)}
              className="mt-1 block h-8 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 text-sm text-[var(--color-text)] focus:border-[var(--color-accent)]"
            />
          </label>
          <button type="submit" disabled={save.isPending} className={QUIET_FIELD}>
            {save.isPending ? text.saving : text.add}
          </button>
          <button
            type="button"
            onClick={() => setAdding(false)}
            className="h-8 rounded px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]"
          >
            {strings.detail.cancel}
          </button>
        </form>
      ) : null}
      {note === null ? null : <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">{note}</p>}
      {reset === undefined || !recordable ? null : (
        <button
          type="button"
          disabled={hand.isPending}
          onClick={() => hand.mutate()}
          className="ease-mechanical mt-2 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {hand.isPending ? strings.materials.resetting : strings.materials.reset}
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
    <section className="mb-8">
      <Label title={strings.fields.title} />
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
const Viewer = lazy(() => import('./Viewer'))

/** The stage's box: the lamp's pool, 10px radius, filling what its parent gives it (DESIGN.md § The stage). */
const LAMP = 'stage-lamp relative h-full min-h-[16rem] w-full overflow-hidden rounded-md'

/**
 * A control on the stage's foot (DESIGN.md § Buttons, Overlay control): a Machined Slate plate with an Edge boundary,
 * never set straight on the lamp, where Edge does not clear 3:1.
 */
const OVERLAY =
  'ease-mechanical pointer-events-auto inline-flex h-8 items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 text-sm font-medium text-[var(--color-text)] duration-[var(--duration-fast)] hover:-translate-y-px hover:text-[var(--color-bright)]'

/**
 * **The part on its stage, and the button that asks for 3D.**
 *
 * Since R1 nothing loads a mesh because a page opened. The stage holds the chosen picture whole (`contain`, A16), or,
 * for the part with no picture — most of them — the still lattice on the lamp with the part's format (A23). **view**
 * is what replaces either with the model, read from the original file in the browser, on the same pool.
 *
 * The button appears only where there is something to draw and something to draw it with: one of the three mesh
 * formats, and WebGL. A pack has no **view** — `mesh.ts` has no loader for it — and keeps its download.
 */
export function Preview({
  part,
  selected = null,
  recordable = false,
}: {
  part: PartDetailData
  /** The picture chosen in the filmstrip; the first one when nothing is, or when the chosen one has gone. */
  selected?: PartImageId | null
  /** Whether the view may be kept as the part's picture: only where the page records changes. */
  recordable?: boolean
}) {
  const queryClient = useQueryClient()
  const [viewing, setViewing] = useState(false)
  // A different part in the same Preview — the quick look steps through the grid without unmounting — closes the
  // view rather than carrying one part's model to the next. So does choosing another picture: it is a request to
  // look at that picture.
  const shownKey = `${part.id}:${selected ?? ''}`
  const [seen, setSeen] = useState(shownKey)
  if (seen !== shownKey) {
    setSeen(shownKey)
    setViewing(false)
  }
  // The same query key the filmstrip uses, so this costs no second request: react-query hands both the one answer.
  const images = useQuery({
    queryKey: ['part-images', part.id],
    queryFn: () => fetchPartImages(part.id),
  })
  const all = images.data ?? []
  const image = all.find((candidate) => candidate.id === selected) ?? all[0] ?? null
  // Pending draws the lamp alone, so a part with a picture never shows the lattice for a moment first.
  const poster = images.isPending ? null : image === null ? (
    <div className="grid h-full w-full place-items-center">
      <Lattice still className="size-[62%]" />
    </div>
  ) : (
    // The whole picture (A16): a stored `cover` is the card's framing, and on a stage it cut a square picture to a band.
    <img src={image.src} alt={strings.parts.pictureAlt(part.name)} className="h-full w-full" style={{ objectFit: 'contain' }} />
  )
  const viewable = part.sourceFormat !== null && MESH_FORMATS.includes(part.sourceFormat)
  const drawable = hasWebGL()
  if (viewing && viewable && drawable) {
    return (
      <Suspense fallback={<div className={LAMP}>{poster}</div>}>
        {/*
          Keyed by part: the quick look and the part page both hand the same Preview a different
          part without unmounting it, and a view kept across parts keeps the last part's framing.
        */}
        <Viewer
          key={part.id}
          part={part}
          poster={poster}
          stage
          // Only for a part with no picture yet: a snapshot joins the end of the gallery, so on a part
          // that has one it would not be "the picture" the button promises.
          keep={
            recordable && images.data?.length === 0
              ? async (png) => {
                  const result = await uploadPartImage(part.id, png, 'snapshot')
                  if (result.kind === 'refused') return result.message
                  await picturesChanged(queryClient, part.id)
                  return null
                }
              : undefined
          }
        />
      </Suspense>
    )
  }
  // The format is the stage's caption when there is no picture (DESIGN.md: "the lamp and the format"); over a
  // picture it is still the one fact about the file the stage can say.
  const format = part.sourceFormat === null ? (image === null ? strings.parts.noPicture(null) : null) : part.sourceFormat.toUpperCase()
  return (
    <figure className={LAMP}>
      {/*
        One layer per picture, crossing over `base` when the filmstrip changes it (DESIGN.md § Motion). `initial`
        off: the first picture is carried in from its card by `flipFrom`, and must not fade under the flight.
      */}
      <Presence initial={false}>
        {poster === null ? null : (
          <Crossfade key={image?.id ?? 'lattice'} className="absolute inset-0">
            {poster}
          </Crossfade>
        )}
      </Presence>
      <figcaption className="pointer-events-none absolute inset-x-3 bottom-3 flex flex-wrap items-end justify-between gap-2">
        {format === null || images.isPending ? <span /> : (
          <span className="tabular rounded-[4px] bg-[var(--color-surface)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--color-muted)]">
            {format}
          </span>
        )}
        {!viewable ? null : drawable ? (
          <button type="button" onClick={() => setViewing(true)} className={OVERLAY}>
            {strings.viewer.view}
          </button>
        ) : (
          <span className="max-w-xs rounded-[var(--radius-ctl)] bg-[var(--color-surface)] px-2 py-1 text-xs text-[var(--color-muted)]">
            {strings.viewer.noWebGL}
          </span>
        )}
      </figcaption>
    </figure>
  )
}

/** The formats a slicer reads as they are, so a part in one downloads for a slicer as its own file (`lapidary-targets`). */
const slicerReads = ['stl', '3mf']

/**
 * `lapidary_core::MESH_EXTENSIONS` — the three formats this library parses, and so the three
 * `mesh.ts` has a loader for. Not `slicerReads`: that is what a slicer can open, a different
 * question with a different answer for `obj`.
 */
const MESH_FORMATS = ['stl', 'obj', '3mf']

const control =
  'ease-mechanical inline-block rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px'

/** The page's one standing control, Download: brighter and heavier than the quiet ones beside it, never blue. */
const standing =
  'ease-mechanical inline-block rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-3 py-1.5 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px'

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
  layout = 'panel',
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
  /** `panel` in the quick look; `page` is the part's own page, laid out as a studio. */
  layout?: 'panel' | 'page'
}) {
  // The info column's blocks arrive in turn when a part's page opens (`arrive`), once per part.
  const head = useRef<HTMLDivElement>(null)
  const info = useRef<HTMLDivElement>(null)
  const lower = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const blocks = [head.current, ...Array.from(info.current?.children ?? [])]
    arrive(blocks.filter((block): block is HTMLElement => block instanceof HTMLElement))
  }, [part.id])
  const page = layout === 'page'
  // `?? []` for a server from before materials, as the list below does: read once, because both the
  // list and the mass row have to agree about how many the part holds.
  const materials = part.materials ?? []
  // The picture on the stage, chosen in the filmstrip; it belongs to one part, so another part starts at its first.
  const [chosen, setChosen] = useState<{ part: PartId; image: PartImageId } | null>(null)
  const selected = chosen?.part === part.id ? chosen.image : null
  const preview = <Preview part={part} selected={selected} recordable={recordable} />
  const heading = (
    <>
      {titled ? null : (
        <h2 className={page ? 'text-2xl leading-tight font-semibold text-[var(--color-bright)]' : 'text-xl font-medium'}>
          {breakable(part.name)}
        </h2>
      )}
      {part.partNumber === null ? null : (
        <p className={page ? 'tabular mt-1.5 text-sm text-[var(--color-dim)]' : 'mt-1 text-sm text-[var(--color-muted)]'}>{part.partNumber}</p>
      )}
    </>
  )
  const tools = (
    <>
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
            className={standing}
          >
            {strings.download.original}
          </a>
        )}
        {/* Quiet beside Download, the one standing control here and in the quick look (DESIGN.md § Buttons). */}
        {part.sourceHash === null ? null : (
          <CopyLink key={part.id} part={part.id} name={part.name} className={control} />
        )}
        {/* Never for a pack (A13): there is no mesh in it to write a 3MF from. */}
        {part.sourceHash !== null &&
        part.sourceFormat !== null &&
        !part.pack &&
        !slicerReads.includes(part.sourceFormat) ? (
          <SlicerExport key={part.revision} part={part} />
        ) : null}
        {/*
          A slot rather than a component, so that what changes the part stays with the
          page and the panel gets only what is safe to show in something transient.
          Removing a model from a dialog that closes on Escape is not a control this
          belongs to.
        */}
        {actions}
      </div>
    </>
  )
  const gallery = (
    // A shared model's pictures are shown and not added to (A4); the owner adds them.
    <Filmstrip
      part={part.id}
      name={part.name}
      editable={recordable}
      selected={selected}
      onSelect={(image) => setChosen({ part: part.id, image })}
    />
  )
  const about = (
    <>
      {/* `?? []` for a server from before tags or materials, which sends a part without them. */}
      <WordList
        part={part}
        recordable={recordable}
        values={part.tags ?? []}
        text={strings.tags}
        save={setPartTags}
        /*
          A tag leads somewhere, here and in the quick look both — this is the one `about` that draws
          them, so the chip is a link wherever a model is shown. Materials get no chip: there is no
          page of a library's materials, and a link that went to one would be a promise we do not keep.
        */
        chip={(value) => <TagLink library={part.library} tag={value} />}
      />
      <WordList
        part={part}
        recordable={recordable}
        values={materials}
        text={strings.materials}
        save={setPartMaterials}
        // Three states, and the note is the only place two of them are visible at all. Typed and
        // empty is a decision — "holds no material" — and it is said even where the list is not
        // editable, because a reader is exactly who needs to know a blank was meant.
        note={
          part.materialsTyped === true && materials.length === 0
            ? strings.materials.none
            : part.materialsTyped === false && materials.length > 0
              ? strings.materials.fromFile
              : null
        }
        reset={part.materialsTyped === true ? unsetPartMaterials : undefined}
      />
      <Fields part={part} recordable={recordable} />

      <Sources part={part.id} recordable={recordable} />
    </>
  )
  // A pack (A13) has no figures: what is inside it takes Geometry's place.
  // Whether any figure here was read off the mesh. The triangle count is marked exactly when they are, so its ≈ always
  // has the key beside it, and a part from before the pivot whose figures are all analytic shows neither.
  const meshFigures = [part.bboxMm, part.volumeMm3, part.surfaceAreaMm2, part.massG].some(
    (figure) => figure?.approximate === true,
  )
  const geometry =
    // Present, not merely non-null: a server from before A13 sends no `pack` at all.
    part.pack ? (
      <PackSection pack={part.pack} />
    ) : (
      <Section
        title={strings.detail.geometry}
        note={
          // The key explains the mark, so it is there exactly when a mark is.
          // `massG` among them: a STEP part's volume is analytic and carries no mark, so a page
          // showing an analytic volume and an approximate mass would print the ≈ with nothing to
          // explain it. A mass is always approximate — see `strings.detail.massSource`.
          meshFigures ? (
            <>
              <span aria-hidden="true" className="mr-1">
                {strings.detail.approximate}
              </span>
              <span>{strings.detail.approximateKey}</span>
            </>
          ) : undefined
        }
      >
        <Row label={strings.detail.triangles} figure>
          {/*
            Read from the mesh, so it carries the mark its neighbours do, as on a card (`meshFigures`).
          */}
          {part.triangleCount === null ? (
            strings.detail.unknown
          ) : (
            <Figure
              figure={{ value: part.triangleCount, approximate: meshFigures }}
              render={strings.detail.trianglesValue}
            />
          )}
        </Row>
        <Row label={strings.detail.boundingBox} figure>
          {part.bboxMm === null ? (
            strings.detail.unknown
          ) : (
            <Figure
              figure={part.bboxMm}
              render={(mm) => strings.detail.boundingBoxValue(mm)}
            />
          )}
        </Row>
        {/* A sentence in place of a figure stays in Archivo (The Copyable Mono Rule). */}
        <Row label={strings.detail.volume} figure={part.volumeMm3 !== null}>
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
        <Row label={strings.detail.surfaceArea} figure>
          {part.surfaceAreaMm2 === null ? (
            strings.detail.unknown
          ) : (
            <Figure figure={part.surfaceAreaMm2} render={strings.detail.surfaceAreaValue} />
          )}
        </Row>
        <Row label={strings.detail.mass} figure={Boolean(part.massG)}>
          {/*
            Volume times the density set for the part's one material. Where there is none, which of
            the four reasons it is gets said: a blank here reads as "weighs nothing", and the reader
            can do something about three of the four. The source sits inside this row's own `dd`
            rather than in a row with an empty `dt`, which is what the `dl` would otherwise become.
          */}
          {/* Falsy and not `=== null`: a server from before L3 sends no `massG` at all, and `Figure`
              cannot be handed `undefined` — it reads the flag off it.

              The reasons are checked in the order a reader can act in. No volume first: an open mesh
              told to set a density would still have no mass after setting one. Then a part somebody
              has said holds no material, which must not be told to give it one — that is the decision
              it just recorded. */}
          {!part.massG ? (
            part.volumeMm3 === null ? (
              strings.detail.massNoVolume
            ) : part.materialsTyped === true && materials.length === 0 ? (
              strings.detail.massHoldsNone
            ) : materials.length === 0 ? (
              strings.detail.massNoMaterial
            ) : materials.length > 1 ? (
              strings.detail.massSeveralMaterials
            ) : (
              strings.detail.massNoDensity(materials[0] as string)
            )
          ) : (
            <>
              <Figure figure={part.massG} render={strings.detail.massValue} />
              <span className="ml-2 text-xs text-[var(--color-muted)]">
                {strings.detail.massSource(materials[0] as string)}
              </span>
            </>
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
  )
  const rest = (
    <>
      <Section id="part-file" title={strings.detail.file}>
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
            : part.fileRoot === LIBRARY || part.fileRoot === INBOX
              ? strings.parts.storedWhere(part.storedBytes, strings.folders.rootFolder[part.fileRoot])
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
      </Section>

      <Section id="part-identity" title={strings.detail.identity}>
        <Row label={strings.detail.sourcePath}>
          <code className="text-xs">{part.sourcePath}</code>
        </Row>
        <Row label={strings.detail.revision}>{part.revLabel}</Row>
      </Section>

      <History part={part.id} />
    </>
  )

  if (!page) {
    return (
      // The quick look: the same stage and filmstrip as the page (A23), then the column's matter in its order.
      <article className="mt-3">
        <div className="h-64">{preview}</div>
        {gallery}
        <div className="mt-1 mb-6">
          {heading}
          {tools}
        </div>
        {geometry}
        {about}
        {rest}
      </article>
    )
  }

  /*
    The part's own page, as a studio: the part on a stage that stays in view on the left, and
    everything about it in a column on the right, read top to bottom in the order a decision needs
    it: what it is, get it, its figures, then what people recorded about it. What is rarely needed
    (the assembly tree, the file's PMI, the file, identity and history) sits under both, in two
    columns, so it is all still on the page and findable, never behind a tab.
  */
  return (
    <article className="mt-2">
      {/*
        The studio is its own grid, closed before the sections under it. The stage is sticky, and a
        sticky box travels as far as its containing block: when that block was the whole page, the
        stage and its pictures slid down over File and Identity as the page scrolled (M1, the owner's
        first look). Now it stops where the studio does.
      */}
      <div className="grid items-start gap-x-8 gap-y-6 lg:grid-cols-[minmax(0,1fr)_22rem] lg:grid-rows-[auto_1fr]">
      {/*
        Source order is reading order on a phone: the part's name and Download, then the stage, then
        the rest. At lg the name moves to the right column beside the stage, which spans both rows.
      */}
      <div ref={head} className="min-w-0 lg:col-start-2 lg:row-start-1">
        {heading}
        {tools}
      </div>
      <div className="min-w-0 lg:sticky lg:top-[4.5rem] lg:col-start-1 lg:row-span-2 lg:row-start-1">
        <div className="h-[min(56vh,24rem)] lg:h-[min(64vh,46rem)] lg:min-h-[20rem]">{preview}</div>
        {gallery}
      </div>
      <div ref={info} className="min-w-0 lg:col-start-2 lg:row-start-2">
        <div>{geometry}</div>
        <div>{about}</div>
      </div>
      </div>
      <div className="mt-6 min-w-0 border-t border-[var(--color-border)] pt-5">
        <SectionIndex within={lower} />
        <div ref={lower} className="mt-5 lg:columns-2 lg:gap-10 [&>*]:break-inside-avoid">
          {rest}
        </div>
      </div>
    </article>
  )
}

/**
 * The part page's index of the sections under the studio: a row of links, not tabs, so every
 * section stays in the page and in a find-in-page.
 *
 * Built from the sections that rendered rather than from a list, because three of them render
 * only for some parts, and two only once their data has arrived: an assembly tree, a file's PMI,
 * a history of more than one revision. A link to a section that is not there is a link that
 * does nothing. It watches its zone for sections arriving and leaving, and takes each one's own
 * heading as its label, so the index cannot name a section differently from the page.
 */
function SectionIndex({ within }: { within: RefObject<HTMLDivElement | null> }) {
  const [links, setLinks] = useState<{ id: string; label: string }[]>([])
  useEffect(() => {
    const zone = within.current
    if (zone === null) return
    const read = () => {
      const next = Array.from(zone.querySelectorAll<HTMLElement>(':scope > section[id]')).map((section) => ({
        id: section.id,
        // The heading's words; its capitals are CSS, so the link reads in sentence case.
        label: section.querySelector('h3')?.textContent ?? section.id,
      }))
      setLinks((current) =>
        current.length === next.length && current.every((link, i) => link.id === next[i]?.id && link.label === next[i]?.label)
          ? current
          : next,
      )
    }
    read()
    const observer = new MutationObserver(read)
    observer.observe(zone, { childList: true, subtree: true })
    return () => observer.disconnect()
  }, [within])
  if (links.length < 2) return null
  return (
    <nav aria-label={strings.detail.sections}>
      <ul role="list" className="flex flex-wrap gap-x-5 gap-y-1">
        {links.map((link) => (
          <li key={link.id}>
            <a
              href={`#${link.id}`}
              // Underlined, in sentence case: the sections below open on uppercase labels, and links
              // styled like them read as more labels rather than as a way to jump.
              className="ease-mechanical text-sm text-[var(--color-dim)] underline decoration-[var(--color-edge)] underline-offset-4 duration-[var(--duration-fast)] hover:text-[var(--color-bright)]"
            >
              {link.label}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  )
}

/**
 * Every revision of the part, newest first (Phase 4 slice 1). Drawn once there is more than
 * one: a history of one revision is the Identity row above it, said twice.
 *
 * Rows only, so the open path — there is no preview per revision to show since R3. Each volume
 * goes through `Figure`, so a mesh-derived figure keeps its ≈ here as everywhere, and each
 * revision's original is a plain download link, byte-identical to what that revision ingested.
 */
function History({ part }: { part: PartId }) {
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
    <section id="part-history" className="mb-8 scroll-mt-20">
      <Label title={strings.detail.history} />
      <ol role="list" className="flex flex-col gap-2 text-sm">
        {all.map((revision) => (
          <li key={revision.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="font-medium">{strings.detail.historyRevision(revision.revLabel)}</span>
            <span className="text-[var(--color-muted)]">
              {strings.detail.origin[revision.origin]} ·{' '}
              {strings.detail.historyDate(revision.createdAt)}
            </span>
            {revision.volumeMm3 === null ? null : (
              <Figure figure={revision.volumeMm3} render={strings.detail.volumeValue} />
            )}
            {revision.onDisk ? (
              <a
                href={downloadUrl(revision.id)}
                download
                className="underline underline-offset-2"
              >
                {strings.download.original}
              </a>
            ) : (
              <span className="text-[var(--color-muted)]" title={strings.detail.historyNotOnDiskWhy}>
                {strings.detail.historyNotOnDisk}
              </span>
            )}
          </li>
        ))}
      </ol>
    </section>
  )
}

/**
 * Copies the part's stable file link. The clipboard is a secure-context API, and a LAN install is
 * plain http, where `navigator.clipboard` is absent — so the fallback is the link itself, selected,
 * for the person to copy, rather than a button that silently does nothing.
 */
function CopyLink({ part, name, className }: { part: PartId; name: string; className: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'manual'>('idle')
  const link = fileLink(part)
  const copied = state === 'copied'
  const manual = state === 'manual'
  const label = copied ? strings.download.linkCopied : strings.download.copyLink
  const announced = copied ? strings.download.linkCopied : ''
  useEffect(() => {
    if (state !== 'copied') return
    const timer = window.setTimeout(() => setState('idle'), 2000)
    return () => window.clearTimeout(timer)
  }, [state])
  return (
    <>
      <button
        type="button"
        aria-label={strings.download.copyLinkFor(name)}
        className={className}
        onClick={() => {
          const clipboard = navigator.clipboard as Clipboard | undefined
          if (clipboard === undefined) {
            setState('manual')
            return
          }
          clipboard.writeText(link).then(
            () => setState('copied'),
            () => setState('manual'),
          )
        }}
      >
        {label}
      </button>
      <span aria-live="polite" className="sr-only">
        {announced}
      </span>
      {manual ? (
        <input
          readOnly
          value={link}
          aria-label={strings.download.linkToCopy}
          ref={(input) => input?.select()}
          onFocus={(event) => event.currentTarget.select()}
          className="w-full max-w-md rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 font-mono text-xs"
        />
      ) : null}
    </>
  )
}


/**
 * What a pack holds (A13), as ingest read it from the archive's index: nothing here opens the file. A RAR,
 * or an index that couldn't be read, says why there is no list.
 */
function PackSection({ pack }: { pack: PackContents }) {
  // A release usually wraps everything in one folder named after itself: said once, above the list.
  const firsts = new Set(pack.entries.map((entry) => entry.path.split('/')[0]))
  const [top] = firsts
  const shared =
    firsts.size === 1 && top !== undefined && pack.entries.every((entry) => entry.path.includes('/')) ? top : null
  const shown = (path: string) => (shared === null ? path : path.slice(shared.length + 1))
  return (
    <Section
      title={strings.detail.pack.title}
      note={pack.unread === null ? strings.detail.pack.count(pack.total) : undefined}
    >
      {pack.unread !== null ? (
        <p className="max-w-prose text-sm text-[var(--color-muted)]">{pack.unread}</p>
      ) : (
        // One child: the section lays its children out side by side.
        <div className="min-w-0">
          {shared === null ? null : (
            <p className="mb-1 text-xs break-all text-[var(--color-muted)]">{strings.detail.pack.inside(shared)}</p>
          )}
          <ul role="list" className="max-h-80 overflow-y-auto rounded-[var(--radius-ctl)] border border-[var(--color-border)] text-sm">
            {pack.entries.map((entry) => (
              <li
                key={entry.path}
                className="flex items-baseline justify-between gap-4 border-b border-[var(--color-border)] px-3 py-1.5 last:border-b-0"
              >
                <span className="min-w-0 break-all">{shown(entry.path)}</span>
                <span className="tabular shrink-0 text-xs text-[var(--color-muted)]">
                  {strings.detail.pack.size(entry.size)}
                </span>
              </li>
            ))}
          </ul>
          {pack.total > pack.entries.length ? (
            <p className="mt-2 text-xs text-[var(--color-muted)]">
              {strings.detail.pack.more(pack.total - pack.entries.length)}
            </p>
          ) : null}
        </div>
      )}
    </Section>
  )
}
/**
 * A section's label over its Scribe Line (DESIGN.md § Layout, page anatomy), with room at its end for the section's
 * one quiet action: an add-form opens from here rather than standing open.
 */
function Label({ title, action }: { title: string; action?: ReactNode }) {
  return (
    <div className="mb-3 flex min-h-6 items-center justify-between gap-3 border-b border-[var(--color-border)] pb-1.5">
      <h3 className="text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">{title}</h3>
      {action}
    </div>
  )
}

/** The quiet Add in a label row: the visible word, and a name that says what it adds. */
function Opener({ name, onOpen }: { name: string; onOpen: () => void }) {
  return (
    <button type="button" aria-label={name} onClick={onOpen} className={QUIET}>
      {strings.detail.add}
    </button>
  )
}

function Section({
  id,
  title,
  note,
  children,
}: {
  /** The anchor the page's section index links to. */
  id?: string
  title: string
  note?: ReactNode
  children: ReactNode
}) {
  return (
    <section id={id} className="mb-8 scroll-mt-20">
      <Label title={title} />
      {note === undefined ? null : (
        <p className="mb-2 text-xs text-[var(--color-muted)]">{note}</p>
      )}
      <dl className="grid grid-cols-[minmax(8rem,max-content)_1fr] gap-x-6 gap-y-1 text-sm">
        {children}
      </dl>
    </section>
  )
}

/** `figure` sets the value in the mono face, as every measurement and count in the interface is. */
function Row({ label, children, figure = false }: { label: string; children: ReactNode; figure?: boolean }) {
  return (
    <>
      <dt className="text-[var(--color-muted)]">{label}</dt>
      <dd className={figure ? 'tabular' : undefined}>{children}</dd>
    </>
  )
}

/** `images` with the one called `id` moved to `index`. */
function moved(images: readonly PartImage[], id: PartImageId, index: number): PartImage[] {
  const rest = images.filter((image) => image.id !== id)
  const image = images.find((candidate) => candidate.id === id)
  if (image === undefined) return [...images]
  rest.splice(Math.max(0, Math.min(index, rest.length)), 0, image)
  return rest
}

/** A quiet button (DESIGN.md § Buttons): transparent, Edge, Graphite 12px, 24px tall. */
const QUIET =
  'ease-mechanical inline-flex h-6 items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px hover:text-[var(--color-text)] disabled:opacity-50'

/** The same, beside a 32px field. */
const QUIET_FIELD = QUIET.replace('h-6', 'h-8')

/**
 * The pictures somebody attached, as a filmstrip directly under the stage (A23, DESIGN.md § The stage and filmstrip),
 * and the controls that attach one, which are the strip's last tile.
 *
 * Each tile is a button that puts its picture on the stage; the arrow keys walk the strip. A15's order stays on the
 * tiles — drag, and the move buttons a keyboard has — and the first picture is still the card's. The card's framing
 * of the chosen picture is set under the strip, because a click on a tile now chooses it.
 *
 * Rendered inside `Detail`, so it appears on the page and in the grid's quick look from one definition. Where
 * nothing can be added and there is at most one picture, there is no strip: the stage already shows it.
 */
function Filmstrip({
  part,
  name,
  editable = true,
  selected,
  onSelect,
}: {
  part: PartId
  name: string
  editable?: boolean
  selected: PartImageId | null
  onSelect: (image: PartImageId) => void
}) {
  const queryClient = useQueryClient()
  const picker = useRef<HTMLInputElement>(null)
  const tiles = useRef<(HTMLButtonElement | null)[]>([])
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
  // size, or a sentence saying what was wrong with what it was given.
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
      void picturesChanged(queryClient, part)
    },
  })

  const gallery = images.data ?? []
  /** The order being dragged into, shown until the server has it; `null` is the gallery as stored. */
  const [draft, setDraft] = useState<PartImage[] | null>(null)
  const [dragged, setDragged] = useState<PartImageId | null>(null)
  const shown = draft ?? gallery
  const ordering = editable && gallery.length > 1
  const reorder = useMutation({
    mutationFn: (next: PartImage[]) => {
      setDraft(next)
      return reorderPartImages(
        part,
        next.map((image) => image.id),
      )
    },
    onSettled: async () => {
      await picturesChanged(queryClient, part)
      setDraft(null)
    },
  })
  const at = Math.max(0, shown.findIndex((image) => image.id === selected))
  const current = shown[at] ?? null
  if (!editable && !images.isError && gallery.length < 2) return null

  /** Left and right walk the strip, Home and End jump to its ends; the stage follows the focus. */
  const walk = (from: number, key: string) => {
    const last = shown.length - 1
    const to = key === 'ArrowLeft' ? from - 1 : key === 'ArrowRight' ? from + 1 : key === 'Home' ? 0 : key === 'End' ? last : null
    if (to === null) return false
    const next = shown[Math.max(0, Math.min(last, to))]
    if (next !== undefined) {
      onSelect(next.id)
      tiles.current[Math.max(0, Math.min(last, to))]?.focus()
    }
    return true
  }

  return (
    <section aria-label={strings.images.title} className="mt-3">
      {/* Same reason as `Sources`: an empty gallery and one we could not read are different. */}
      {images.isError ? (
        <p role="alert" className="mb-2 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.images.galleryFailed}
        </p>
      ) : null}
      <ul role="list" title={ordering ? strings.images.orderHint : undefined} className="-m-1 flex list-none items-start gap-2 overflow-x-auto overscroll-x-contain p-1">
        {shown.map((image, index) => {
          const label = strings.images.alt(name, index)
          const chosen = index === at
          return (
            <li
              key={image.id}
              draggable={ordering}
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = 'move'
                setDragged(image.id)
              }}
              onDragOver={(event) => {
                if (dragged === null) return
                event.preventDefault()
                if (dragged !== image.id) setDraft(moved(shown, dragged, index))
              }}
              onDrop={(event) => {
                event.preventDefault()
                if (draft !== null) reorder.mutate(draft)
                setDragged(null)
              }}
              onDragEnd={(event) => {
                setDragged(null)
                // Let go outside the strip: the order goes back to the stored one.
                if (event.dataTransfer.dropEffect === 'none') setDraft(null)
              }}
              className={`flex w-16 flex-none flex-col ${dragged === image.id ? 'opacity-50' : ''}`}
            >
              {/*
                The tile's name is the picture's, so the image inside is decoration to a reader: the one
                `images.alt` image on the page is the framing control's, under the strip.
              */}
              <button
                ref={(button) => {
                  tiles.current[index] = button
                }}
                type="button"
                aria-label={label}
                aria-current={chosen ? 'true' : undefined}
                title={image.sourceUrl === null ? undefined : strings.images.from(image.sourceUrl)}
                onClick={() => onSelect(image.id)}
                onKeyDown={(event) => {
                  if (walk(index, event.key)) event.preventDefault()
                }}
                className={`block size-16 overflow-hidden rounded-[var(--radius-ctl)] border bg-[var(--color-raised)] hover:border-[var(--color-edge)] ${chosen ? 'border-[var(--color-edge)]' : 'border-[var(--color-border)]'}`}
              >
                <img src={image.src} alt="" draggable={false} className="h-full w-full" style={pictureStyle(image)} />
              </button>
              {/* The current one's mark: a 2px Quicklime underline, a position as well as a colour. */}
              <span
                aria-hidden="true"
                className={`mx-auto mt-1 h-0.5 w-8 rounded-full ${chosen ? 'bg-[var(--color-bright)]' : 'bg-transparent'}`}
              />
              {!ordering ? null : (
                <div className="flex items-center justify-between">
                  <button
                    type="button"
                    aria-label={strings.images.moveEarlier(label)}
                    disabled={index === 0 || reorder.isPending}
                    onClick={() => reorder.mutate(moved(shown, image.id, index - 1))}
                    className="rounded-[var(--radius-sm)] p-1 text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-50"
                  >
                    <Icon name="caretLeft" size={12} />
                  </button>
                  <button
                    type="button"
                    aria-label={strings.images.moveLater(label)}
                    disabled={index === shown.length - 1 || reorder.isPending}
                    onClick={() => reorder.mutate(moved(shown, image.id, index + 1))}
                    className="rounded-[var(--radius-sm)] p-1 text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-50"
                  >
                    <Icon name="caretRight" size={12} />
                  </button>
                </div>
              )}
              {index !== 0 || !ordering ? null : (
                <span className="text-center text-[10px] leading-tight text-[var(--color-muted)]">{strings.images.onCard}</span>
              )}
            </li>
          )
        })}
        {!editable ? null : (
          <li className="flex h-16 flex-none flex-col justify-center gap-1.5">
            {/*
              Hidden, and driven by the button beside it: a bare file input is unstyleable across
              browsers and announces itself as "Choose file", which is not what this does.
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
            <button type="button" onClick={() => picker.current?.click()} disabled={add.isPending} className={QUIET}>
              {add.isPending ? strings.images.adding : strings.images.add}
            </button>
            {url !== null ? null : (
              <button
                type="button"
                onClick={() => {
                  setRefusal(null)
                  setUrl('')
                }}
                className={QUIET}
              >
                {strings.images.addFromUrl}
              </button>
            )}
          </li>
        )}
      </ul>
      {!reorder.isError ? null : (
        <p role="alert" className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
          {strings.images.reorderFailed}
        </p>
      )}
      {/*
        A real `<form>`, so Enter submits it — pasting an address and pressing Enter is what
        a person will do. The refusals that matter are about where the address points, and
        only the server can know those.
      */}
      {url === null ? null : (
        <form
          className="mt-2 flex max-w-lg flex-wrap items-center gap-2"
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
            onKeyDown={(event) => {
              if (event.key === 'Escape') setUrl(null)
            }}
            placeholder={strings.images.urlPlaceholder}
            aria-label={strings.images.urlLabel}
            className="h-8 min-w-0 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 text-xs"
          />
          <button type="submit" disabled={add.isPending || url.trim() === ''} className={QUIET}>
            {add.isPending ? strings.images.fetching : strings.images.fetch}
          </button>
          <button type="button" onClick={() => setUrl(null)} className="h-6 rounded px-2 text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]">
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
      {!editable || current === null ? null : (
        <Framed key={current.id} part={part} image={current} label={strings.images.alt(name, at)} />
      )}
    </section>
  )
}
