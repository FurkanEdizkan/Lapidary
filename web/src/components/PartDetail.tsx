import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useRef, useState, type ReactNode } from 'react'
import { blobUrl, downloadUrl, fetchPartImages, uploadPartImage } from '../lib/api'
import { strings } from '../lib/strings'
import type { Approximate, PartDetail as PartDetailData, PartId } from '../lib/types'

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
export function Detail({ part, actions }: { part: PartDetailData; actions?: ReactNode }) {
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
            <a
              href={downloadUrl(part.revision)}
              download
              className="ease-mechanical inline-block rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
            >
              {strings.download.original}
            </a>
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
    <section className="mb-6">
      <h3 className="mb-2 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {title}
      </h3>
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

  const images = useQuery({
    queryKey: ['part-images', part],
    queryFn: () => fetchPartImages(part),
  })

  const add = useMutation({
    mutationFn: (file: File) => uploadPartImage(part, file),
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
      void queryClient.invalidateQueries({ queryKey: ['part-images', part] })
    },
  })

  const gallery = images.data ?? []
  return (
    <section className="mb-6">
      <h3 className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.images.title}
      </h3>
      {gallery.length === 0 ? null : (
        <ul className="mb-2 flex list-none flex-wrap gap-2">
          {gallery.map((image, index) => (
            <li key={image.id}>
              <img
                src={image.src}
                alt={strings.images.alt(name, index)}
                title={image.sourceUrl === null ? undefined : strings.images.from(image.sourceUrl)}
                className="h-24 w-24 rounded border border-[var(--color-border)] bg-[var(--color-surface)] object-cover"
              />
            </li>
          ))}
        </ul>
      )}
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
        className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {add.isPending ? strings.images.adding : strings.images.add}
      </button>
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
