import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { freeRenderCache } from '../lib/api'
import { Dialog } from './Dialog'
import { strings } from '../lib/strings'
import type { InstanceStorageView, LibraryStorage } from '../lib/types'

/**
 * What the whole library occupies, under the page that shows part of it.
 *
 * Rendered only where the grid has cards: a line of zeroes over an empty library says
 * nothing the empty state has not already said better. Both totals are bytes on disk —
 * source files counted one per part, derivatives deduplicated, and `PgParts::storage_totals`
 * is where that accounting is written down — and the ratio arrives computed rather than
 * divided here, so a second reader cannot report the same library the other way up.
 */
export function StorageTotals({ storage, isError }: { storage?: LibraryStorage; isError: boolean }) {
  if (isError) {
    return <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">{strings.storage.failed}</p>
  }
  // Nothing at all while the first read is in flight: a total is a claim about the
  // library, and there is no honest placeholder for a claim.
  if (storage === undefined) {
    return null
  }
  return (
    <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
      {strings.storage.totals(storage.sourceBytes, storage.derivativeBytes, storage.derivativeRatio)}
      {/*
        Only when there is something to say. A permanent "0 B removed" would be noise on
        every library that has never removed anything, which is most of them — but the
        moment one exists, the totals above stop describing the whole volume and this is
        what says so.
      */}
      {storage.removedBytes > 0 ? strings.storage.removed(storage.removedBytes) : null}
    </p>
  )
}

/**
 * What the whole store holds, under the one library's line.
 *
 * Separate from `StorageTotals` and not folded into it, because it answers a different
 * question and the two disagree on purpose: a derivative two libraries share is charged to
 * both of them above and counted once here, and the quarantined figure belongs to no
 * library at all. Adding the panels up is exactly the thing this is here to stop somebody
 * doing.
 *
 * The disk measurement is a button rather than part of the load. It costs the server a
 * `stat` per file — instant on a small library, seconds on a corpus — and the four figures
 * beside it are free, so making everyone pay for it on every page load to answer a question
 * most visits do not ask would be the wrong default.
 */
export function InstanceStorage({
  instance,
  isError,
  measuring,
  measured,
  onMeasure,
}: {
  instance?: InstanceStorageView
  isError: boolean
  /** The walk is in flight. */
  measuring: boolean
  /** The walk has been asked for, whether or not it came back. */
  measured: boolean
  onMeasure: () => void
}) {
  const queryClient = useQueryClient()
  const [asking, setAsking] = useState(false)
  const free = useMutation({
    mutationFn: freeRenderCache,
    onSuccess: () => {
      setAsking(false)
      void queryClient.invalidateQueries({ queryKey: ['instance-storage'] })
    },
  })
  // Same rule as the library totals: nothing at all while the first read is in flight,
  // because a total is a claim and there is no honest placeholder for one.
  if (isError) {
    return <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.storage.failed}</p>
  }
  if (instance === undefined) {
    return null
  }

  const {
    sourceBytes,
    derivativeBytes,
    inlinePreviewBytes,
    removedBytes,
    quarantinedBytes,
    renderCacheBytes,
    onDiskBytes,
  } = instance
  // Deliberately **without** `inlinePreviewBytes`: those are in Postgres, and this figure
  // is compared against a walk of the storage folder. Including them put the tracked total
  // above the disk by exactly their size on a real library, which reads as loss.
  const tracked = sourceBytes + derivativeBytes + removedBytes + quarantinedBytes
  // Narrowed here and not in the JSX, for the reason `ShowInFolder` narrows where it does:
  // a `typeof x === 'number'` inside a child expression puts the literal `'number'` in a
  // position `no-bare-strings.test.ts` reads — correctly — as a label reaching the screen.
  // `typeof` and not truthiness, because a genuinely empty store measures 0, which is an
  // answer rather than a missing one.
  const onDisk = typeof onDiskBytes === 'number' ? onDiskBytes : null
  return (
    <div className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">
      <p>
        {strings.storage.everything(
          sourceBytes,
          derivativeBytes,
          inlinePreviewBytes,
          removedBytes,
          quarantinedBytes,
        )}
      </p>
      {free.data !== undefined ? (
        <p className="mt-1">{strings.storage.cacheFreed(free.data.removed, free.data.quarantinedBytes)}</p>
      ) : renderCacheBytes > 0 ? (
        <p className="mt-1 flex flex-wrap items-center gap-2">
          <span>{strings.storage.renderCache(renderCacheBytes)}</span>
          <button
            type="button"
            onClick={() => setAsking(true)}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.storage.freeCache}
          </button>
        </p>
      ) : null}
      {!asking ? null : (
        <Dialog title={strings.storage.freeCacheTitle} onClose={() => setAsking(false)}>
          <p className="mt-3 text-sm">{strings.storage.freeCacheBody(renderCacheBytes)}</p>
          {free.isError ? (
            <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
              {strings.storage.freeCacheFailed}
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
              disabled={free.isPending}
              onClick={() => free.mutate()}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
            >
              {strings.storage.freeCacheConfirm}
            </button>
          </div>
        </Dialog>
      )}
      {onDisk !== null ? (
        <p className="mt-1">{strings.storage.onDisk(onDisk, tracked)}</p>
      ) : measuring ? (
        <p className="mt-1">{strings.storage.measuring}</p>
      ) : measured ? (
        // Asked for and not answered: the walk failed, and the four figures above are still
        // true because they never came from the disk.
        <p className="mt-1">{strings.storage.onDiskFailed}</p>
      ) : (
        <button
          type="button"
          onClick={onMeasure}
          className="ease-mechanical mt-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.storage.measureOnDisk}
        </button>
      )}
    </div>
  )
}
