import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { fetchHostFolders, makeHostFolder } from '../lib/api'
import { strings } from '../lib/strings'
import { DIALOG_FOOT, DIALOG_QUIET, Dialog } from './Dialog'
import { Icon } from './Icon'
import { STANDING } from './Page'

/** The path bar's own controls: borderless, because they sit in a row of them and the row is the boundary. */
const QUIET =
  'ease-mechanical rounded-[var(--radius-ctl)] px-2 py-1 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-50'
const FIELD =
  'min-w-0 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 font-mono text-[13px] text-[var(--color-text)]'
const ROW =
  'flex w-full items-center gap-2 border-b border-[var(--color-border)] px-3 py-1.5 text-left text-sm last:border-b-0'

/**
 * The folder picker (A6, a dialog since A9): the machine's folders one at a time, reached by name, by the path
 * bar's segments or by typing a path, and chosen with "Use this folder" when Lapidary may write there. New folder
 * makes one inside the folder shown and opens it. A folder it can't open or write says what to run.
 * `code` is the setup code in the wizard, before anybody can sign in.
 */
export function FolderPicker({
  title,
  start,
  code = null,
  onChoose,
  onCancel,
}: {
  title: string
  start: string | null
  code?: string | null
  onChoose: (path: string) => void
  onCancel: () => void
}) {
  const queryClient = useQueryClient()
  const [at, setAt] = useState<string | null>(start)
  const [typed, setTyped] = useState<string | null>(null)
  const [naming, setNaming] = useState<string | null>(null)
  const listing = useQuery({
    queryKey: ['host-folders', at, code],
    queryFn: () => fetchHostFolders(at, code),
    // The folder you came from stays on screen while the next one is read, rather than a flash of nothing.
    placeholderData: keepPreviousData,
  })
  const data = listing.isError ? undefined : listing.data
  const open = (path: string) => {
    setTyped(null)
    setNaming(null)
    make.reset()
    setAt(path)
  }
  const make = useMutation({
    mutationFn: (name: string) => makeHostFolder(data?.path ?? '', name, code),
    onSuccess: (made) => {
      queryClient.setQueryData(['host-folders', made.path, code], made)
      void queryClient.invalidateQueries({ queryKey: ['host-folders'] })
      open(made.path)
    },
  })

  const usable = data?.choosable === true && !listing.isPlaceholderData
  const root = data?.root ?? '/'
  const base = root.replace(/\/$/, '')
  const below = data ? data.path.slice(base.length).split('/').filter(Boolean) : []
  const crumb = (i: number) => `${base}/${below.slice(0, i + 1).join('/')}`
  // The end of a long path is the part that says where you are.
  const field = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (field.current !== null && typed === null) field.current.scrollLeft = field.current.scrollWidth
  }, [data?.path, typed])

  return (
    <Dialog title={title} onClose={onCancel} size="wide">
      <form
        className="mt-3 flex gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          // A portal passes React events up its owner's tree: the wizard's own form would submit too.
          event.stopPropagation()
          if (typed !== null && typed.trim() !== '') open(typed.trim())
        }}
      >
        <input
          ref={field}
          aria-label={strings.folderPicker.pathLabel}
          value={typed ?? data?.path ?? at ?? ''}
          onChange={(event) => setTyped(event.target.value)}
          spellCheck={false}
          className={FIELD}
        />
        <button type="submit" disabled={typed === null} className={DIALOG_QUIET}>
          {strings.folderPicker.go}
        </button>
      </form>

      <div className="mt-3 flex items-center gap-1">
        <button
          type="button"
          aria-label={strings.folderPicker.up}
          title={strings.folderPicker.up}
          disabled={!data?.parent}
          onClick={() => data?.parent && open(data.parent)}
          className={QUIET}
        >
          <Icon name="arrowUp" />
        </button>
        <nav
          aria-label={strings.folderPicker.crumbs}
          className="flex min-w-0 flex-1 items-center overflow-x-auto text-sm"
        >
          <button
            type="button"
            title={root}
            aria-current={below.length === 0 ? 'location' : undefined}
            onClick={() => open(root)}
            className={`${QUIET} whitespace-nowrap ${below.length === 0 ? 'text-[var(--color-text)]' : ''}`}
          >
            {base === '' ? '/' : base.slice(base.lastIndexOf('/') + 1)}
          </button>
          {below.map((name, i) => (
            <span key={crumb(i)} className="flex min-w-0 items-center">
              {i > 0 || base !== '' ? (
                <span aria-hidden="true" className="text-[var(--color-dim)]">
                  /
                </span>
              ) : null}
              <button
                type="button"
                aria-current={i === below.length - 1 ? 'location' : undefined}
                onClick={() => open(crumb(i))}
                className={`${QUIET} whitespace-nowrap ${i === below.length - 1 ? 'text-[var(--color-text)]' : ''}`}
              >
                {name}
              </button>
            </span>
          ))}
        </nav>
        <button
          type="button"
          disabled={data === undefined || data.problem !== null || naming !== null}
          onClick={() => setNaming('')}
          className={`${QUIET} flex shrink-0 items-center gap-1.5`}
        >
          <Icon name="folderPlus" />
          {strings.folderPicker.newFolder}
        </button>
      </div>

      <div className="mt-1 h-[min(45vh,22rem)] overflow-y-auto overscroll-contain rounded-[var(--radius-ctl)] border border-[var(--color-edge)]">
        {naming !== null ? (
          <form
            className={`${ROW} bg-[var(--color-raised)]`}
            onSubmit={(event) => {
              event.preventDefault()
              event.stopPropagation()
              if (naming.trim() !== '') make.mutate(naming.trim())
            }}
          >
            <Icon name="folder" />
            <input
              autoFocus
              aria-label={strings.folderPicker.newName}
              value={naming}
              disabled={make.isPending}
              onChange={(event) => setNaming(event.target.value)}
              onKeyDown={(event) => {
                // Escape leaves the name, not the whole picker.
                if (event.key === 'Escape') {
                  event.stopPropagation()
                  setNaming(null)
                  make.reset()
                }
              }}
              className={FIELD}
            />
            <button type="submit" disabled={naming.trim() === '' || make.isPending} className={DIALOG_QUIET}>
              {make.isPending ? strings.folderPicker.making : strings.folderPicker.make}
            </button>
            <button
              type="button"
              onClick={() => {
                setNaming(null)
                make.reset()
              }}
              className={QUIET}
            >
              {strings.folderPicker.cancel}
            </button>
          </form>
        ) : null}
        {make.isError ? (
          <p role="alert" className="border-b border-[var(--color-border)] px-3 py-2 text-sm">
            {make.error.message}
          </p>
        ) : null}
        {listing.isPending ? (
          <p className="p-3 text-sm text-[var(--color-muted)]">{strings.folderPicker.loading}</p>
        ) : listing.isError ? (
          <p role="alert" className="p-3 text-sm text-[var(--color-text)]">
            {listing.error.message || strings.folderPicker.failed}
          </p>
        ) : data?.folders.length === 0 && naming === null ? (
          <p className="p-3 text-sm text-[var(--color-dim)]">{strings.folderPicker.empty}</p>
        ) : (
          <ul role="list" aria-label={strings.folderPicker.listLabel}>
            {data?.folders.map((name) => (
              <li key={name}>
                <button
                  type="button"
                  aria-label={strings.folderPicker.open(name)}
                  onClick={() => open(`${data.path.replace(/\/$/, '')}/${name}`)}
                  className={`${ROW} ease-mechanical duration-[var(--duration-fast)] hover:bg-[var(--color-raised)]`}
                >
                  <span className="shrink-0 text-[var(--color-muted)]">
                    <Icon name="folder" />
                  </span>
                  <span className="truncate">{name}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {data?.problem ? (
        <p role="status" className="mt-2 text-xs break-words text-[var(--color-muted)]">
          {data.problem}
        </p>
      ) : null}
      <div className={DIALOG_FOOT}>
        <button type="button" onClick={onCancel} className={DIALOG_QUIET}>
          {strings.folderPicker.cancel}
        </button>
        <button
          type="button"
          disabled={!usable}
          onClick={() => data && onChoose(data.path)}
          className={usable ? STANDING : DIALOG_QUIET}
        >
          {strings.folderPicker.use}
        </button>
      </div>
    </Dialog>
  )
}
