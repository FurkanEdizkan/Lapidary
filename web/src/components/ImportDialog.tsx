import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  addSource,
  addSourcePicture,
  browseModels,
  fetchImportSettings,
  fetchSources,
  removeSource,
} from '../lib/api'
import { strings } from '../lib/strings'
import type { LibraryId } from '../lib/types'
import { DIALOG_FOOT, DIALOG_QUIET, DIALOG_SECTION, Dialog } from './Dialog'
import { LEAD, SECTION_TITLE, STANDING } from './Page'

/** Words that act — a crumb, Remove, another way in: Graphite, underlined under the pointer. */
const QUIET =
  'ease-mechanical text-sm text-[var(--color-muted)] underline-offset-2 duration-[var(--duration-fast)] hover:text-[var(--color-text)] hover:underline'
/** Between two folders of the path; not copy, so not in `strings.ts`. */
const SEPARATOR = '/'
/** A field's label over it: sentence case, Graphite. */
const FIELD_LABEL = 'flex flex-col gap-1 text-xs text-[var(--color-muted)]'
const ROW = 'flex min-h-8 items-center gap-2 border-b border-[var(--color-border)] px-2 last:border-b-0'

/**
 * The Import dialog (A2, owner 2026-10-17): a file browser over the models folder. Tick files or
 * folders — or add the folder you are in — optionally with the model's page and pictures, and they
 * are added to this library and scanned where they are. Nothing is copied.
 *
 * The order is the server's (`added.rs`): the source first, then its pictures, then one scan, so a
 * picture is there before the part it is for. Upload and a page import stay as they were, offered
 * at the foot.
 */
export function ImportDialog({
  library,
  onClose,
  onScan,
  onUpload,
  onUploadFolder,
  onFromLink,
}: {
  library: LibraryId
  onClose: () => void
  /** Scan the library, with the grid's own progress line. */
  onScan: () => void
  /** Files, archives among them (A10). */
  onUpload: () => void
  onUploadFolder: () => void
  onFromLink: () => void
}) {
  const queryClient = useQueryClient()
  const [path, setPath] = useState('')
  const [picked, setPicked] = useState<string[]>([])
  const [pageUrl, setPageUrl] = useState('')
  const [fetchDetails, setFetchDetails] = useState<boolean | null>(null)
  const [pictures, setPictures] = useState<File[]>([])
  const [note, setNote] = useState<string | null>(null)

  const listing = useQuery({ queryKey: ['browse', path], queryFn: () => browseModels(path) })
  const sources = useQuery({ queryKey: ['sources', library], queryFn: () => fetchSources(library) })
  const settings = useQuery({ queryKey: ['import-settings'], queryFn: fetchImportSettings })
  const fetching = fetchDetails ?? settings.data?.fetchDetails ?? false

  const add = useMutation({
    mutationFn: async (paths: string[]) => {
      const page = pageUrl.trim()
      for (const at of paths) {
        const source = await addSource(library, {
          path: at,
          pageUrl: page === '' ? null : page,
          fetchDetails: page !== '' && fetching,
        })
        for (const picture of pictures) await addSourcePicture(library, source.id, picture)
      }
      return paths.length
    },
    onSuccess: (count) => {
      setNote(strings.addModels.added(count))
      setPicked([])
      setPictures([])
      setPageUrl('')
      void queryClient.invalidateQueries({ queryKey: ['sources', library] })
      onScan()
    },
    onError: (error) => setNote(error.message),
  })
  const remove = useMutation({
    mutationFn: (source: string) => removeSource(library, source),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['sources', library] })
      void queryClient.invalidateQueries({ queryKey: ['parts'] })
    },
    onError: (error) => setNote(error.message),
  })

  const crumbs = path === '' ? [] : path.split('/')
  const toggle = (at: string) =>
    setPicked((now) => (now.includes(at) ? now.filter((p) => p !== at) : [...now, at]))

  return (
    <Dialog title={strings.addModels.title} onClose={onClose}>
      <p className={LEAD}>{strings.addModels.lead}</p>

      <nav aria-label={strings.addModels.top} className="mt-4 flex flex-wrap items-center gap-1 text-sm">
        <button type="button" onClick={() => setPath('')} className={QUIET}>
          {strings.addModels.top}
        </button>
        {crumbs.map((name, i) => (
          <span key={crumbs.slice(0, i + 1).join('/')} className="flex items-center gap-1">
            <span aria-hidden="true" className="text-[var(--color-dim)]">
              {SEPARATOR}
            </span>
            <button type="button" onClick={() => setPath(crumbs.slice(0, i + 1).join('/'))} className={QUIET}>
              {name}
            </button>
          </span>
        ))}
      </nav>

      <div className="mt-2 max-h-[40vh] overflow-y-auto overscroll-contain rounded-[var(--radius-ctl)] border border-[var(--color-edge)]">
        {listing.isPending ? (
          <p className="p-3 text-sm text-[var(--color-muted)]">{strings.addModels.loading}</p>
        ) : listing.isError ? (
          <p role="alert" className="p-3 text-sm text-[var(--color-muted)]">
            {listing.error.message || strings.addModels.failed}
          </p>
        ) : listing.data.entries.length === 0 ? (
          <p className="p-3 text-sm text-[var(--color-muted)]">{strings.addModels.empty}</p>
        ) : (
          <ul role="list">
            {listing.data.entries.map((entry) => (
              <li key={entry.path} className={ROW}>
                <input
                  type="checkbox"
                  checked={picked.includes(entry.path)}
                  onChange={() => toggle(entry.path)}
                  aria-label={strings.addModels.select(entry.name)}
                  className="size-4 accent-[var(--color-dim)]"
                />
                {entry.isFolder ? (
                  <button
                    type="button"
                    onClick={() => setPath(entry.path)}
                    aria-label={strings.addModels.open(entry.name)}
                    className="flex-1 truncate py-1.5 text-left text-sm font-semibold text-[var(--color-bright)] hover:underline"
                  >
                    {entry.name}
                  </button>
                ) : (
                  <span className="flex-1 truncate py-1.5 text-sm">{entry.name}</span>
                )}
                {entry.sizeBytes === null ? null : (
                  <span className="tabular text-[11px] text-[var(--color-dim)]">
                    {strings.addModels.size(entry.sizeBytes)}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      {listing.data?.truncated ? (
        <p className="mt-1 text-xs text-[var(--color-dim)]">{strings.addModels.truncated}</p>
      ) : null}

      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        <label className={FIELD_LABEL}>
          {strings.addModels.pageLabel}
          <input
            type="url"
            inputMode="url"
            value={pageUrl}
            onChange={(event) => setPageUrl(event.target.value)}
            autoComplete="url"
            spellCheck={false}
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm text-[var(--color-text)]"
          />
          <span className="text-[11px] text-[var(--color-muted)]">{strings.addModels.pageHint}</span>
        </label>
        {/*
          No raw native widget (DESIGN.md § Dialogs): the input is visually hidden and keeps its label, and the
          button-shaped span is the label's own text, so a click on it opens the chooser and the input's focus
          ring is drawn on the span.
        */}
        <label className={FIELD_LABEL}>
          {strings.addModels.picturesLabel}
          <input
            type="file"
            accept="image/*"
            multiple
            onChange={(event) => setPictures(Array.from(event.target.files ?? []))}
            className="peer sr-only"
          />
          <span
            aria-hidden="true"
            className={`${DIALOG_QUIET} cursor-pointer self-start py-1.5 peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-[var(--color-accent)]`}
          >
            {strings.importPictures.choose}
          </span>
          <span className="text-[11px] text-[var(--color-muted)]">
            {pictures.length === 0 ? strings.importPictures.none : strings.addModels.picturesChosen(pictures.length)}
          </span>
        </label>
      </div>
      <label className="mt-3 flex min-h-6 items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={fetching}
          disabled={pageUrl.trim() === ''}
          onChange={(event) => setFetchDetails(event.target.checked)}
          className="size-4 accent-[var(--color-dim)]"
        />
        {strings.addModels.fetchDetails}
      </label>

      {note === null ? null : (
        <p role="status" className="mt-3 text-sm text-[var(--color-text)]">
          {note}
        </p>
      )}

      <div className={DIALOG_FOOT}>
        <button type="button" disabled={add.isPending} onClick={() => add.mutate([path])} className={DIALOG_QUIET}>
          {path === '' ? strings.addModels.whole : strings.addModels.addThis}
        </button>
        <button
          type="button"
          disabled={add.isPending || picked.length === 0}
          onClick={() => add.mutate(picked)}
          // Standing only once it can act: "Add 3 selected", never "Add 0 selected" (DESIGN.md § Dialogs).
          className={add.isPending || picked.length === 0 ? DIALOG_QUIET : STANDING}
        >
          {add.isPending ? strings.addModels.adding : strings.addModels.add(picked.length)}
        </button>
      </div>

      <section className={DIALOG_SECTION}>
        <h3 className={SECTION_TITLE}>{strings.addModels.inLibrary}</h3>
        {sources.data === undefined || sources.data.length === 0 ? (
          <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.addModels.nothingAdded}</p>
        ) : (
          <ul role="list" className="mt-2">
            {sources.data.map((source) => (
              <li key={source.id} className="flex items-center gap-2 py-1 text-sm">
                <span className="flex-1 truncate font-mono text-[13px]">
                  {source.path === '' ? strings.addModels.whole : source.path}
                </span>
                <button
                  type="button"
                  disabled={remove.isPending}
                  onClick={() => remove.mutate(source.id)}
                  title={strings.addModels.removeTitle(source.path)}
                  className={QUIET}
                >
                  {strings.addModels.remove}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <p className="mt-4 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-[var(--color-muted)]">
        {strings.addModels.otherWays}
        <button type="button" onClick={onUpload} className={QUIET}>
          {strings.addModels.upload}
        </button>
        <button type="button" onClick={onUploadFolder} className={QUIET}>
          {strings.addModels.uploadFolder}
        </button>
        <button type="button" onClick={onFromLink} className={QUIET}>
          {strings.addModels.fromLink}
        </button>
      </p>
    </Dialog>
  )
}
