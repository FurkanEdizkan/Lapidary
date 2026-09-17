import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState, type RefObject } from 'react'
import { fetchFailures, retryFailed } from '../lib/api'
import { strings } from '../lib/strings'
import { filesFromDrop, filesFromInput } from '../lib/upload'
import type { PickedFile, UploadProgress } from '../lib/upload'
import type { BatchId, BatchStatus, JobFailure, JobId, LibraryId } from '../lib/types'
import { Icon } from './Icon'

/**
 * Jobs the worker is finished with, however it finished with them.
 *
 * Subtraction, not a sum of the outcome counters. Summing them was a standing bug the
 * moment a new outcome appeared: `rendered` was once missing from the sum, so a thumbnail
 * sweep — every job of which settles as `rendered` — returned 0 for the whole batch, the
 * invalidation below never fired, and the grid stayed blank while every job succeeded.
 * `scanned` would be the same bug a second time. What is actually being asked is "how
 * much is no longer in flight", and `total - pending - running` answers exactly that for
 * every outcome there will ever be, including the ones `BatchStatus` does not break out.
 *
 * Named for jobs rather than files since a `derive` job is a revision and a
 * `scan_directory` job is a directory.
 */
export function jobsSettled(status: BatchStatus): number {
  return status.total - status.pending - status.running
}

/**
 * Which copy the progress line uses. `BatchStatus` carries counters, not job kinds, so
 * this is read from two things instead: a batch this page started is whichever kind the
 * click that started it was — the state below carries that alongside the id, and it has
 * to, now that this page can start a scan, a render or a migration. Failing that, a
 * batch is read by what it CONTAINS: any `migrate_storage` row at all means a migration,
 * else any settled `derive` job means a render, else it is read as a scan. This is what
 * a batch this page never clicked into falls back to: a sweep or a migration started
 * with `curl`, or a migration the worker queued on its own at startup and nothing on
 * this page ever asked for.
 *
 * Migration is checked on `migrating` (rows of that kind), not `migrated` (settled
 * outcomes), and checked first. `migrated` stays 0 until a `migrate_storage` job
 * actually finishes, but the worker's startup enqueue is the ONLY way a migration batch
 * can exist, so every migration a browser can watch starts at `migrating: 1, migrated:
 * 0` — reading `migrated` here would misreport every migration as a scan for the whole
 * duration of its first run, which is worst on exactly the large corpus where that run
 * is slowest. `render` has no rows-based equivalent yet and keeps reading `rendered`
 * (settled outcomes), so it keeps the same one-job blind window `migrate` used to have —
 * a pre-existing gap this task does not extend, not one it closes.
 *
 * What this reads at all is a batch mixing more than one kind: nothing enqueues one —
 * `enqueue` is called once per payload kind, a scan's own children are all
 * `ingest_file`, and a migration's are all `migrate_storage`.
 */
export type BatchKind = 'scan' | 'render' | 'migrate' | 'upload'

function progressText(status: BatchStatus, kind: BatchKind): string {
  if (status.finishedAt === null) {
    const settled = jobsSettled(status)
    if (kind === 'render') {
      return strings.render.running(settled, status.total)
    }
    if (kind === 'migrate') {
      return strings.migrate.running
    }
    // No walk job in an upload's batch, so nothing to subtract and no walk to wait for.
    if (kind === 'upload') {
      return strings.upload.batchRunning(settled, status.total)
    }
    // `total` counts jobs and the walk is one of them, so both halves are shifted by the
    // number of walks that have finished. While that is still 0 the batch holds nothing
    // but the walk, and there is no file count to report yet — the worker is still
    // reading the directory.
    if (status.scanned === 0) {
      return strings.scan.walking
    }
    return strings.scan.running(settled - status.scanned, status.total - status.scanned)
  }
  if (kind === 'render') {
    return strings.render.finished(status.rendered)
  }
  if (kind === 'migrate') {
    // The failure count, so the sentence can stop claiming every file arrived when the
    // line beside it says some did not. `scan.finished` and `render.finished` both report
    // what happened and let the failure line qualify them; this used to assert a total.
    return strings.migrate.finished(status.failedTotal)
  }
  if (kind === 'upload') {
    return strings.upload.batchFinished(
      status.ingested,
      status.skipped,
      status.revised,
      status.unkept,
    )
  }
  return strings.scan.finished(status.ingested, status.skipped, status.revised, status.unkept)
}

/**
 * Where a folder goes in: anywhere on the page, while a drag carries files.
 *
 * Both gestures, because they are not interchangeable. `<input webkitdirectory>` opens
 * the picker and reports `webkitRelativePath` for free; a *drag* of a folder gives
 * neither — `DataTransfer.files` holds the folder as a zero-byte entry with none of its
 * contents — so the drop path goes through `webkitGetAsEntry` and a recursive walk. See
 * `filesFromDrop`, which is also where the `readEntries` 100-entry limit is handled.
 *
 * **The whole window is the target, and nothing marks it at rest.** A dashed strip above the
 * grid spent a row of every visit on a gesture used on a few. The overlay appears only once a
 * drag carrying `Files` enters the window; a card dragged onto a category carries the part's
 * own type and no files, so moving a part never raises it.
 *
 * `dragover` must call `preventDefault` or the browser navigates to the dropped file instead of
 * handing it here, which looks exactly like a broken page. The counter, rather than a boolean:
 * `dragenter`/`dragleave` fire for every element the pointer crosses, so a boolean flickers off
 * the moment the drag passes over anything.
 */
export function DropOverlay({
  onFiles,
  picker,
}: {
  onFiles: (picked: PickedFile[]) => void
  /** The hidden folder input, opened by the header's Upload button. */
  picker: RefObject<HTMLInputElement | null>
}) {
  const [depth, setDepth] = useState(0)
  // The latest handler without re-subscribing four window listeners on every render.
  const handler = useRef(onFiles)
  useEffect(() => {
    handler.current = onFiles
  })

  useEffect(() => {
    const carriesFiles = (event: DragEvent) => event.dataTransfer?.types.includes('Files') ?? false
    const enter = (event: DragEvent) => {
      if (!carriesFiles(event)) return
      event.preventDefault()
      setDepth((d) => d + 1)
    }
    const over = (event: DragEvent) => {
      if (carriesFiles(event)) event.preventDefault()
    }
    const leave = (event: DragEvent) => {
      if (carriesFiles(event)) setDepth((d) => Math.max(0, d - 1))
    }
    const drop = (event: DragEvent) => {
      if (!carriesFiles(event) || event.dataTransfer === null) return
      event.preventDefault()
      setDepth(0)
      void filesFromDrop(event.dataTransfer.items).then((picked) => handler.current(picked))
    }
    window.addEventListener('dragenter', enter)
    window.addEventListener('dragover', over)
    window.addEventListener('dragleave', leave)
    window.addEventListener('drop', drop)
    return () => {
      window.removeEventListener('dragenter', enter)
      window.removeEventListener('dragover', over)
      window.removeEventListener('dragleave', leave)
      window.removeEventListener('drop', drop)
    }
  }, [])

  return (
    <>
      {depth === 0 ? null : (
        /*
          Layout Blue, because this is the one moment the page is live for a drop. The frame is
          inset from the window edge so it reads as a target and not as a border on the browser.
        */
        <div
          data-drop-overlay
          className="scrim-in fixed inset-0 z-[var(--z-overlay)] bg-[var(--color-bg)]/95 p-4"
        >
          <div className="grid size-full place-items-center rounded-md border-2 border-dashed border-[var(--color-accent)]">
            <p className="flex items-center gap-2 rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3 text-base font-medium text-[var(--color-bright)]">
              <Icon name="upload" size={20} />
              {strings.upload.dropNow}
            </p>
          </div>
        </div>
      )}
      {/*
        `webkitdirectory` is not in React's typed attribute set — it is a non-standard
        attribute every browser that matters implements — so it is spelled lowercase as a
        DOM attribute rather than camelCased. Hidden rather than styled: the Upload button is
        the control, and a bare file input cannot be made to look like anything.
      */}
      <input
        ref={picker}
        type="file"
        multiple
        {...{ webkitdirectory: '' }}
        hidden
        onChange={(event) => {
          if (event.target.files !== null) {
            onFiles(filesFromInput(event.target.files))
          }
          // So picking the same folder twice in a row fires a second change event.
          event.target.value = ''
        }}
      />
    </>
  )
}

/** How far through its current phase an upload is, 0 to 1. */
export function uploadFraction(progress: UploadProgress): number {
  switch (progress.phase) {
    case 'hashing':
      return progress.filesTotal === 0 ? 0 : progress.filesDone / progress.filesTotal
    case 'probing':
      return 0
    case 'transferring':
      return progress.bytesToSend === 0 ? 1 : progress.bytesSent / progress.bytesToSend
    case 'committing':
      return 1
  }
}

/**
 * The header's one standing button.
 *
 * Quiet at rest: an edge and no accent. Layout Blue marks what is live (DESIGN.md), and an
 * Upload button waiting for a click is not; an upload running is, so the accent arrives as the
 * bar along its foot and a mono percentage for the phase in hand. The sentence saying which
 * phase stays in the page's status line, because a percentage alone cannot say "hashing".
 */
export function UploadButton({
  onUpload,
  busy,
  progress,
}: {
  onUpload: () => void
  busy: boolean
  progress: UploadProgress | undefined
}) {
  const fraction = busy && progress !== undefined ? uploadFraction(progress) : null
  return (
    <button
      type="button"
      onClick={onUpload}
      disabled={busy}
      className="ease-mechanical relative flex min-h-8 flex-none items-center gap-2 overflow-hidden rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-3 text-xs font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:hover:translate-y-0"
    >
      <Icon name="upload" />
      <span className="max-sm:sr-only">{strings.toolbar.upload}</span>
      {fraction === null ? null : (
        <>
          <span aria-hidden="true" className="tabular text-[11px] font-normal text-[var(--color-muted)]">
            {Math.round(fraction * 100)}%
          </span>
          <span
            aria-hidden="true"
            style={{ transform: `scaleX(${fraction})` }}
            className="absolute inset-x-0 bottom-0 h-0.5 origin-left bg-[var(--color-accent)]"
          />
        </>
      )}
    </button>
  )
}

/** One line for whichever phase the upload is in. Each phase has a different number
 *  worth showing, which is why this is a switch and not one string with holes in it. */
export function progressLine(progress: UploadProgress): string {
  switch (progress.phase) {
    case 'hashing':
      return strings.upload.hashing(progress.filesDone, progress.filesTotal)
    case 'probing':
      return strings.upload.probing
    case 'transferring':
      return strings.upload.transferring(
        progress.bytesSent,
        progress.bytesToSend,
        progress.filesDone,
        progress.filesTotal,
      )
    case 'committing':
      return strings.upload.committing
  }
}

/**
 * The batch line: how far a batch has got, how it ended, and what went wrong.
 *
 * The reasons, not only the count. `batch_status` has returned a `failures` list carrying
 * each job's `last_error` since slice 2 and nothing displayed it, which mattered more
 * once the directory walk moved into a job: an unreadable `/ingest` mount used to fail
 * the request an operator was watching, and now fails a job in this batch. If the reason
 * does not reach the screen, that move only relocated a terminal round-trip somewhere
 * less obvious. See `crates/lapidary-ingest/src/scan.rs`'s module doc.
 *
 * Nothing renders while the first poll is in flight. A batch whose status has not
 * arrived yet is not a fact about the library, and the grid below is the page — a
 * placeholder here would push it down for one tick and then move it back.
 */
export function ScanProgress({
  status,
  isError,
  kind,
  library,
  batch,
}: {
  status?: BatchStatus
  isError: boolean
  kind: BatchKind
  library: LibraryId
  batch: BatchId
}) {
  const queryClient = useQueryClient()
  // Failures past the hundred the status carries, fetched only when somebody asks for them.
  const [more, setMore] = useState<JobFailure[]>([])
  const showMore = useMutation({
    mutationFn: (after: JobId) => fetchFailures(library, batch, after),
    onSuccess: (page) => setMore((held) => [...held, ...page.failed]),
  })
  // The server reopens the batch, so reading its status again restarts the poll, which stops
  // only on a finished batch. The pages fetched past the sample are dropped rather than
  // patched: the retried rows are no longer failures, and the status re-lists what still is.
  const retry = useMutation({
    mutationFn: (job: JobId | undefined) => retryFailed(library, batch, job),
    onSuccess: () => {
      setMore([])
      void queryClient.invalidateQueries({ queryKey: ['batch', library, batch] })
    },
  })
  // Picked once, out here: a `kind === 'render' ? … : …` inside JSX puts the discriminator
  // itself in a child expression, where `no-bare-strings.test.ts` reads it — correctly —
  // as a bare literal reaching the screen.
  //
  // Every kind, and `migrate` is not a fall-through to `scan`'s copy. A failed
  // migration used to render "3 files could not be read. They will not appear in the grid"
  // about models that already exist and are already in the grid, and a status poll that
  // failed rendered "No scan with that id has run in this library" to an operator who
  // never started a scan. A non-destructive failure worded as data loss is the one class of
  // mistake this product treats as a correctness bug.
  const copy =
    kind === 'render'
      ? strings.render
      : kind === 'migrate'
        ? strings.migrate
        : kind === 'upload'
          ? // Scan's per-file failure line, which never says "scan", and an unknown line of its
            // own: `scan.unknown` tells somebody who never started one about a scan.
            { failed: strings.scan.failed, unknown: strings.upload.batchUnknown }
          : strings.scan
  if (isError) {
    return <p className="mb-4 max-w-prose text-[var(--color-muted)]">{copy.unknown}</p>
  }
  if (status === undefined) {
    return null
  }
  // The server caps the list at 100 while `failedTotal` is the real number, so a batch
  // with more failures than that says so rather than trailing off at the hundredth — and the
  // count is a button, so the rest is a press away.
  const listed = [...status.failed, ...more]
  const hidden = status.failedTotal - listed.length
  const last = listed[listed.length - 1]
  // A migration retries itself (see `PgJobs::retry`), so its line offers nothing to press.
  const retryable = kind !== 'migrate'
  const button =
    'ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-60'
  return (
    <div className="mb-4 max-w-prose text-[var(--color-muted)]">
      <p className="flex flex-wrap items-center gap-2">
        <span>{progressText(status, kind)}</span>
        {status.failedTotal === 0 ? null : <span>{copy.failed(status.failedTotal)}</span>}
        {retryable && status.failedTotal > 1 ? (
          <button
            type="button"
            className={button}
            disabled={retry.isPending}
            onClick={() => retry.mutate(undefined)}
          >
            {strings.failure.retryAll(status.failedTotal)}
          </button>
        ) : null}
      </p>
      {retry.isError ? (
        <p role="alert" className="mt-2 text-sm">
          {strings.failure.retryFailed}
        </p>
      ) : null}
      {listed.length === 0 ? null : (
        <ul role="list" className="mt-2 space-y-1 text-sm">
          {listed.map((failure) => (
            // The job, which is the one thing unique to a row: two jobs can name the same
            // file, and a `scan_directory` failure has no path at all.
            <li key={failure.job} className="flex flex-wrap items-baseline gap-2">
              <span>{strings.failure.line(failure.path, failure.reason)}</span>
              {retryable ? (
                <button
                  type="button"
                  className={button}
                  aria-label={strings.failure.retryOne(failure.path)}
                  disabled={retry.isPending}
                  onClick={() => retry.mutate(failure.job)}
                >
                  {strings.failure.retry}
                </button>
              ) : null}
            </li>
          ))}
          {hidden <= 0 || last === undefined ? null : (
            <li>
              <button
                type="button"
                className={button}
                disabled={showMore.isPending}
                onClick={() => showMore.mutate(last.job)}
              >
                {strings.failure.more(hidden)}
              </button>
            </li>
          )}
        </ul>
      )}
      {showMore.isError ? (
        <p role="alert" className="mt-2 text-sm">
          {strings.failure.moreFailed}
        </p>
      ) : null}
    </div>
  )
}
