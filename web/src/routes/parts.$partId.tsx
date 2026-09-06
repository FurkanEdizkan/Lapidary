import { Link, createFileRoute, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { DEFAULT_LIBRARY_ID, blobUrl, downloadUrl, fetchPartDetail, removePart } from '../lib/api'
import { strings } from '../lib/strings'
import type { Approximate, PartDetail } from '../lib/types'

/**
 * One part, in full — the second route this app has.
 *
 * The grid answers "which of these is the one I want". This answers "is this one right",
 * which wants figures a card has no room for: the box the part has to fit in, whether the
 * mesh is closed, what the file actually is, and the path that is its identity.
 *
 * # Every figure renders through the same component, and that is the point
 *
 * `Figure` takes an `Approximate<T>` and cannot render the value without consulting the
 * flag beside it. `CLAUDE.md`: *"Mesh-derived measurements are labelled 'approximate' in
 * the UI, always."* A page-level badge would be the easy thing and it is wrong from Phase
 * 2 on — a STEP part carries an analytic volume next to a tessellated triangle count on
 * one revision, and one badge is then wrong about one figure whichever way it is set.
 */
export const Route = createFileRoute('/parts/$partId')({
  component: RouteComponent,
})

/**
 * Reads the path param and hands it to `PartPage` as a prop, the same split `index.tsx`
 * makes with its search param: the component that does the work takes plain props, so a
 * test can drive it without spelling a URL.
 */
function RouteComponent() {
  const { partId } = Route.useParams()
  return <PartPage partId={partId} />
}

export function PartPage({ partId }: { partId: string }) {
  const part = useQuery({
    queryKey: ['part', partId],
    queryFn: () => fetchPartDetail(partId),
  })

  return (
    <section>
      <Link
        to="/"
        className="ease-mechanical text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
      >
        {strings.detail.back}
      </Link>
      {part.isPending ? (
        <p className="mt-6 text-[var(--color-muted)]">{strings.detail.loading}</p>
      ) : part.isError ? (
        <p className="mt-6 max-w-prose text-[var(--color-muted)]">{strings.detail.failed}</p>
      ) : (
        <Detail part={part.data} />
      )}
    </section>
  )
}

function Detail({ part }: { part: PartDetail }) {
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
            <Remove part={part} />
          </div>
          {/*
            The reassurance sits beside the button rather than behind a confirmation
            dialog. Removing is reversible and touches nothing on disk, so a modal would
            spend on this action the alarm that purge is going to need — and purge is one
            deliberate step further away, on the removed list this sends you to.
          */}
          <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
            {strings.removal.removeHint}
          </p>
        </div>
      </header>

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
function Remove({ part }: { part: PartDetail }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const remove = useMutation({
    mutationFn: () => removePart(part.id),
    onSuccess: async () => {
      // Both lists change: this part leaves the grid and joins the removed list. Awaited
      // so the navigation lands on a grid that has already dropped the card, rather than
      // showing it for one frame and then blinking it away.
      await queryClient.invalidateQueries({ queryKey: ['parts', DEFAULT_LIBRARY_ID] })
      await navigate({ to: '/' })
    },
  })

  return (
    <>
      <button
        type="button"
        onClick={() => remove.mutate()}
        disabled={remove.isPending}
        className="ease-mechanical rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {remove.isPending ? strings.removal.removing : strings.removal.remove}
      </button>
      {remove.isError ? (
        <span className="text-xs text-[var(--color-muted)]">{strings.removal.removeFailed}</span>
      ) : null}
    </>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
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

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-[var(--color-muted)]">{label}</dt>
      <dd>{children}</dd>
    </>
  )
}
