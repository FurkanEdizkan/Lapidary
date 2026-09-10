import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useRef, useState, type ReactNode } from 'react'
import {
  addPartSource,
  blobUrl,
  downloadUrl,
  fetchPartImages,
  fetchPartSources,
  setImageFraming,
  uploadPartImage,
} from '../lib/api'
import { strings } from '../lib/strings'
import type { Approximate, PartDetail as PartDetailData, PartId, PartImage } from '../lib/types'

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
export function Detail({
  part,
  actions,
  recordable = false,
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
}) {
  return (
    <article className="mt-4">
      <header className="mb-6 flex flex-wrap items-start gap-6">
        {part.thumbnail === null ? null : (
          <img
            src={part.thumbnail}
            alt={strings.parts.thumbnailAlt(part.name)}
            className="h-40 w-40 rounded border border-[var(--color-border)] bg-[var(--color-surface)] object-contain"
          />
        )}
        <div>
          <h2 className="text-xl font-medium">{part.name}</h2>
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
            {/*
              A slot rather than a component, so that what changes the part stays with the
              page and the panel gets only what is safe to show in something transient.
              Removing a model from a dialog that closes on Escape is not a control this
              belongs to.
            */}
            {actions}
          </div>
        </div>
      </header>

      <Gallery part={part.id} name={part.name} />

      <Sources part={part.id} recordable={recordable} />

      <Section title={strings.detail.geometry}>
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
            <code className="text-xs">{part.sourceHash}</code>
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
        <Row label={strings.detail.kernel}>
          {part.kernelVersion === null ? strings.detail.unknown : part.kernelVersion}
        </Row>
      </Section>
    </article>
  )
}

/**
 * A measured value and its provenance, which cannot be rendered apart.
 *
 * The badge is on the figure rather than on the page because provenance is per figure:
 * `Approximate<T>` carries the flag with the value precisely so a caller cannot show one
 * without the other, and this is that type reaching the screen.
 */
function Figure<T>({
  figure,
  render,
}: {
  figure: Approximate<T>
  render: (value: T) => string
}) {
  return (
    <span
      title={figure.approximate ? strings.detail.approximateTitle : strings.detail.exactTitle}
    >
      {render(figure.value)}
      {figure.approximate ? (
        <span className="ml-1 text-[var(--color-muted)]">{strings.detail.approximate}</span>
      ) : null}
    </span>
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

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    /*
      The container is declared here, on the section, and queried on the `<dl>` inside it.
      A container query never matches the element that *declares* the container — only its
      descendants — so `@container/rows` and `@min-[19rem]/rows:` on one element is a query
      that silently never fires, which is what the first attempt at this did.
    */
    <section className="@container/rows mb-6">
      <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {title}
      </h3>
      {/*
        Two columns where there is room for two, one where there is not.

        `Detail` renders on the part's own page — a wide column — and inside the inspector
        rail, which is 340px. The fixed `minmax(8rem,max-content)` label track was sized for
        the first and left about 160px for a value in the second, which is not enough for a
        BLAKE3 prefix or for "197 kB on disk, compressed from 610 kB": both ran off the edge
        of the rail rather than wrapping.

        A container query and not a viewport one, because the thing that varies is the
        *container* — the rail is narrow on a 2,560px screen, and `md:` would give it two
        columns there and one on a phone, which is backwards in both cases.
      */}
      <dl className="grid grid-cols-1 gap-x-4 gap-y-1 text-sm @min-[19rem]/rows:grid-cols-[minmax(6.5rem,max-content)_1fr] @sm/rows:gap-x-6">
        {children}
      </dl>
    </section>
  )
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-[var(--color-muted)]">{label}</dt>
      {/*
        `min-w-0` so the value track may actually shrink — a grid track's default minimum is
        `auto`, which is the content's own minimum size, so a long unbroken string pushes the
        track wider than the column rather than wrapping inside it. `anywhere` rather than
        `break-word` because the string this is for is a hex digest: it has no break
        opportunity at all, and `overflow-wrap: break-word` will not create one mid-"word"
        when the word already starts a line.
      */}
      <dd className="min-w-0 [overflow-wrap:anywhere]">{children}</dd>
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
