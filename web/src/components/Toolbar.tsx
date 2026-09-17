import { Link } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { createLibrary, DEFAULT_LIBRARY_ID, fetchLibraries, makeControlled } from '../lib/api'
import { Dialog, closeMenu } from './Dialog'
import { FieldsMenuItem } from './Fields'
import { DensitiesMenuItem } from './Densities'
import {
  DENSITIES,
  LAYOUTS,
  PAGE_SIZES,
  SORTS,
  type Density,
  type Layout,
  type PageSize,
  type Sort,
} from '../lib/preferences'
import { strings } from '../lib/strings'
import type { LibraryId, NewLibrary } from '../lib/types'
import { Menu } from './Menu'

/**
 * Everything above the grid, in one row.
 *
 * `v2` puts navigation, search, view options and upload in a single bar. This used to be four
 * stacked rows — the action bar, the library switcher, the grid settings and the search field
 * — which put 387px of chrome between the page's top and its first render. The controls are
 * the same controls; what changed is that the ones you set once and leave (card size, page
 * size, which library, whether previews render) went into two menus, and the ones you touch
 * every visit (search, upload, switching to the removed list) stayed on the bar.
 *
 * **Two bars, not one, and that is a deliberate difference from the design.** The brand sits
 * in the root route's header and this row sits under it. Hoisting search into the root would
 * move its debounce and its `q` navigation out of the route that owns that search param, and
 * `index.test.tsx` drives this route's own `SearchBox` through a synthetic root — so the
 * refactor would rewrite a large share of that file to save one 38px row.
 *
 * Menus are native popovers (`styles.css` has why). Every control inside one stays in the
 * document while the menu is closed, which is what keeps a test that finds the auto-thumbnail
 * checkbox by role meaningful — and also what would let such a test pass if the menu could
 * never be opened at all, so `index.test.tsx` asserts the trigger is wired to its menu, and
 * the keyboard pass in the browser is the check that it opens.
 */
export function Toolbar({
  autoThumbnail,
  onAutoThumbnail,
  settingsBusy,
  settingsNote,
  onScan,
  scanBusy,
  onSweep,
  sweepBusy,
  note,
  library,
  onSelectLibrary,
  pageSize,
  onPageSize,
  density,
  onDensity,
  layout,
  onLayout,
  sort,
  onSort,
  searching,
  selecting,
  onSelecting,
  onUpload,
  uploadBusy,
  onImport,
  importBusy,
  search,
}: {
  autoThumbnail: boolean | undefined
  onAutoThumbnail: (on: boolean) => void
  settingsBusy: boolean
  settingsNote: string | null
  onScan: () => void
  scanBusy: boolean
  onSweep: () => void
  sweepBusy: boolean
  note: string | null
  library: LibraryId
  onSelectLibrary?: (library: LibraryId) => void
  pageSize: PageSize
  onPageSize: (size: PageSize) => void
  density: Density
  onDensity: (density: Density) => void
  layout: Layout
  onLayout: (layout: Layout) => void
  sort: Sort
  onSort: (sort: Sort) => void
  /** A search is running, and a search is in relevance order whatever `sort` says. */
  searching: boolean
  selecting: boolean
  onSelecting: (on: boolean) => void
  onUpload: () => void
  uploadBusy: boolean
  /** Opens the picker for a bundle exported from Lapidary. */
  onImport: () => void
  importBusy: boolean
  /** The search field, built by the route that owns its query. */
  search: ReactNode
}) {
  return (
    <>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        {/*
          Grid is where you are, so it is marked and not linked: a link to `/` from `/` would
          drop the category and the search you are in the middle of. Removed is a place, not
          an action, and the only route back to a part somebody removed — so it stays a link,
          carrying the library, as it was.
        */}
        <nav
          aria-label={strings.toolbar.views}
          className="flex flex-none items-center gap-0.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-raised)] p-[3px]"
        >
          <span
            aria-current="page"
            className="flex min-h-6 items-center rounded-[5px] bg-[var(--color-surface)] px-3 text-xs font-semibold text-[var(--color-bright)]"
          >
            {strings.toolbar.grid}
          </span>
          <Link
            to="/removed"
            search={library === DEFAULT_LIBRARY_ID ? undefined : { library }}
            className="ease-mechanical flex min-h-6 items-center rounded-[5px] px-3 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
          >
            {strings.removal.removedTitle}
          </Link>
          {/* The whole installation's, not one library's, so it carries no library the way Removed does. */}
          <Link
            to="/sharing"
            className="ease-mechanical flex min-h-6 items-center rounded-[5px] px-3 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
          >
            {strings.sharing.title}
          </Link>
        </nav>
        {search}
        <button
          type="button"
          aria-pressed={selecting}
          onClick={() => onSelecting(!selecting)}
          className="ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] aria-pressed:border-[var(--color-accent)] aria-pressed:text-[var(--color-bright)]"
        >
          {strings.selection.toggle}
        </button>
        <Menu id="view-menu" label={strings.toolbar.view}>
          <p className="text-xs font-medium text-[var(--color-muted)]">{strings.toolbar.layout}</p>
          <div role="group" aria-label={strings.toolbar.layout} className="flex gap-1.5">
            {LAYOUTS.map((option) => (
              <button
                key={option}
                type="button"
                aria-pressed={layout === option}
                onClick={() => onLayout(option)}
                className={
                  layout === option
                    ? 'min-h-6 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-accent)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-bright)]'
                    : 'ease-mechanical min-h-6 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]'
                }
              >
                {LAYOUT_LABEL[option]}
              </button>
            ))}
          </div>
          <label className="flex items-center justify-between gap-3 text-xs text-[var(--color-muted)]">
            {strings.grid.sort}
            <select
              value={sort}
              disabled={searching}
              aria-describedby={searching ? 'sort-while-searching' : undefined}
              onChange={(event) => onSort(event.target.value as Sort)}
              className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 disabled:opacity-60"
            >
              {SORTS.map((option) => (
                <option key={option} value={option}>
                  {strings.grid.sortOption[option]}
                </option>
              ))}
            </select>
          </label>
          {searching && (
            <p id="sort-while-searching" className="text-xs text-[var(--color-muted)]">
              {strings.grid.sortWhileSearching}
            </p>
          )}
          <label className="flex items-center justify-between gap-3 text-xs text-[var(--color-muted)]">
            {strings.grid.density}
            <select
              value={density}
              onChange={(event) => onDensity(event.target.value as Density)}
              className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
            >
              {DENSITIES.map((option) => (
                <option key={option} value={option}>
                  {DENSITY_LABEL[option]}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center justify-between gap-3 text-xs text-[var(--color-muted)]">
            {strings.grid.pageSize}
            <select
              value={pageSize}
              onChange={(event) => onPageSize(Number(event.target.value) as PageSize)}
              className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
            >
              {PAGE_SIZES.map((size) => (
                <option key={size} value={size}>
                  {strings.grid.pageSizeOption(size)}
                </option>
              ))}
            </select>
          </label>
        </Menu>
        <Menu id="library-menu" label={strings.toolbar.library}>
          <LibrarySwitcher library={library} onSelect={onSelectLibrary} />
        <label className="flex items-center gap-2 text-sm" title={strings.library.autoThumbnailDetail}>
          {/*
            `undefined` is "not known yet", and the checkbox says so in the way a checkbox
            says it: mixed, and not clickable until there is a state to click away from.
            Painting a confident "on" for the tick before the read lands is the same lie in a
            shorter window, and a box that flips under the cursor is worse than one that
            waits. It stays mixed if the read fails outright — `settingsNote` says why and
            says to reload — because there is nothing honest to put there.

            `indeterminate` is a DOM property with no attribute, so it is set through the ref
            rather than rendered. Block body: a React 19 ref callback that returns a value is
            read as a cleanup function.
          */}
          <input
            type="checkbox"
            checked={autoThumbnail ?? false}
            ref={(el) => {
              if (el !== null) {
                el.indeterminate = autoThumbnail === undefined
              }
            }}
            disabled={settingsBusy || autoThumbnail === undefined}
            onChange={(event) => onAutoThumbnail(event.target.checked)}
            className="accent-[var(--color-accent)]"
          />
          {strings.library.autoThumbnail}
        </label>
          <button
            type="button"
            onClick={onScan}
            disabled={scanBusy}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.scan.start}
          </button>
          <button
            type="button"
            onClick={onSweep}
            disabled={sweepBusy}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.render.sweep}
          </button>
          <FieldsMenuItem library={library} />
          <DensitiesMenuItem library={library} />
        </Menu>
        <button
          type="button"
          onClick={onUpload}
          disabled={uploadBusy}
          className="ease-mechanical flex min-h-6 flex-none items-center rounded-[var(--radius-ctl)] border border-[var(--color-accent)] bg-[var(--color-surface)] px-3 py-1.5 text-xs font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {strings.toolbar.upload}
        </button>
        <button
          type="button"
          onClick={onImport}
          disabled={importBusy}
          className="ease-mechanical flex min-h-6 flex-none items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {strings.toolbar.importBundle}
        </button>
      </div>
      {/*
        Outside the menus, always. A note that says the scan failed or that previews could not
        be queued answers a control that may live in a closed menu, and a message inside a
        closed menu is a message nobody reads.
      */}
      {settingsNote === null && note === null ? null : (
        <p className="mb-3 flex flex-wrap gap-x-4 text-sm text-[var(--color-muted)]">
          {settingsNote === null ? null : <span>{settingsNote}</span>}
          {note === null ? null : <span>{note}</span>}
        </p>
      )}
    </>
  )
}

/**
 * Which library this screen is of, and the control that makes another one.
 *
 * Hidden entirely while there is one library — which is every deployment until somebody
 * makes a second. A switcher offering one choice is a control that explains nothing and
 * takes a row of the screen to do it; the "New library" button stays, because that is how
 * the second one gets made.
 */
function LibrarySwitcher({
  library,
  onSelect,
}: {
  library: LibraryId
  onSelect?: (library: LibraryId) => void
}) {
  const [creating, setCreating] = useState(false)
  const queryClient = useQueryClient()
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  const [refusal, setRefusal] = useState<string | null>(null)

  const add = useMutation({
    mutationFn: (body: NewLibrary) => createLibrary(body),
    onMutate: () => setRefusal(null),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setRefusal(result.message)
        return
      }
      setCreating(false)
      void queryClient.invalidateQueries({ queryKey: ['libraries'] })
      // Straight into it: somebody who just made a library meant to use it, and leaving
      // them on the old one is a second step for no reason.
      onSelect?.(result.library.id)
    },
  })

  // The one-way switch a changed file in a hobby library points to. Offered only on a hobby
  // library, behind a dialog, because it cannot be undone.
  const [switching, setSwitching] = useState(false)
  const keepChanges = useMutation({
    mutationFn: () => makeControlled(library),
    onSuccess: () => {
      setSwitching(false)
      void queryClient.invalidateQueries({ queryKey: ['libraries'] })
    },
  })

  const all = libraries.data ?? []
  const current = all.find((one) => one.id === library)
  const offerSwitch = current?.mode === 'hobby'
  return (
    <div className="mb-3 flex flex-wrap items-center gap-3 text-xs text-[var(--color-muted)]">
      {all.length < 2 ? null : (
        <label className="flex items-center gap-2">
          {strings.libraries.label}
          <select
            value={library}
            onChange={(event) => onSelect?.(event.target.value as LibraryId)}
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
          >
            {all.map((one) => (
              <option key={one.id} value={one.id}>
                {strings.libraries.option(one.name, one.partCount)}
              </option>
            ))}
          </select>
        </label>
      )}
      <button
        type="button"
        onClick={(event) => {
          closeMenu(event.currentTarget)
          setCreating(true)
        }}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.libraries.create}
      </button>
      {!offerSwitch ? null : (
        <button
          type="button"
          onClick={(event) => {
            closeMenu(event.currentTarget)
            setSwitching(true)
          }}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.libraries.makeControlled}
        </button>
      )}
      {!switching || current === undefined ? null : (
        <Dialog title={strings.libraries.makeControlledTitle} onClose={() => setSwitching(false)}>
          <p className="mt-3 text-sm">{strings.libraries.makeControlledBody(current.name)}</p>
          {keepChanges.isError ? (
            <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
              {strings.libraries.makeControlledFailed}
            </p>
          ) : null}
          <div className="mt-4 flex justify-end gap-2">
            <button
              type="button"
              autoFocus
              onClick={() => setSwitching(false)}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
            >
              {strings.folders.cancel}
            </button>
            <button
              type="button"
              disabled={keepChanges.isPending}
              onClick={() => keepChanges.mutate()}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
            >
              {strings.libraries.makeControlledConfirm}
            </button>
          </div>
        </Dialog>
      )}
      {libraries.isError ? <span role="alert">{strings.libraries.failed}</span> : null}
      {!creating ? null : (
        <NewLibraryDialog
          busy={add.isPending}
          note={refusal ?? (add.isError ? strings.libraries.createFailed : null)}
          onConfirm={(body) => add.mutate(body)}
          onCancel={() => {
            setCreating(false)
            setRefusal(null)
          }}
        />
      )}
    </div>
  )
}

/**
 * A name and a governance mode, chosen once.
 *
 * The mode is at creation because later means asking about a library somebody has already
 * filled; a hobby library can still be switched afterwards, one way, from `LibrarySwitcher`.
 * `CLAUDE.md`: governance is opt-in per library, and `controlled` is what keeps revisions.
 */
function NewLibraryDialog({
  busy,
  note,
  onConfirm,
  onCancel,
}: {
  busy: boolean
  note: string | null
  onConfirm: (body: NewLibrary) => void
  onCancel: () => void
}) {
  const [name, setName] = useState('')
  const [mode, setMode] = useState<NewLibrary['mode']>('hobby')
  const trimmed = name.trim()
  return (
    <Dialog title={strings.libraries.createTitle} onClose={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          if (trimmed !== '' && !busy) onConfirm({ name: trimmed, mode })
        }}
      >
        <input
          type="text"
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-label={strings.libraries.nameLabel}
          autoFocus
          className="mt-3 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
        />
        <label className="mt-3 flex flex-col gap-1 text-xs text-[var(--color-muted)]">
          {strings.libraries.modeLabel}
          <select
            value={mode}
            onChange={(event) => setMode(event.target.value as NewLibrary['mode'])}
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
          >
            {LIBRARY_MODES.map((option) => (
              <option key={option} value={option}>
                {MODE_LABEL[option]}
              </option>
            ))}
          </select>
        </label>
        {note === null ? null : (
          <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
            {note}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.folders.cancel}
          </button>
          <button
            type="submit"
            disabled={busy || trimmed === ''}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.libraries.createConfirm}
          </button>
        </div>
      </form>
    </Dialog>
  )
}

/** The two modes, and their labels — a lookup rather than a ternary in JSX, for the reason
 * `DENSITY_LABEL` above gives. */
const LIBRARY_MODES = ['hobby', 'controlled'] as const

const MODE_LABEL: Record<(typeof LIBRARY_MODES)[number], string> = {
  hobby: strings.libraries.hobby,
  controlled: strings.libraries.controlled,
}

/**
 * How many cards a page holds, and how tightly they pack.
 *
 * Two `<select>`s and no custom widget: a native select is keyboard-operable, screen-reader
 * announced and correct on a touch screen for free, and this is a preference rather than a
 * place to spend design on.
 *
 * Both are remembered per library in this browser — not on the server. There is no user
 * table and no auth in Phase 1, so a column would make one operator's choice everybody's;
 * `FEATURES.md` says "per viewer, per library" now, because that is what this is.
 */
/**
 * The label for each density. A lookup and not a ternary in the JSX: a `option === 'compact'`
 * inside a child expression puts the literal `'compact'` where `no-bare-strings.test.ts`
 * reads it — correctly — as a label reaching the screen. Same trap `ShowInFolder` and
 * `InstanceStorage` both carry a comment about.
 */
const LAYOUT_LABEL: Record<Layout, string> = {
  detail: strings.layouts.detail,
  gallery: strings.layouts.gallery,
  list: strings.layouts.list,
}

const DENSITY_LABEL: Record<Density, string> = {
  comfortable: strings.grid.comfortable,
  compact: strings.grid.compact,
}

/**
 * The key that moves focus into search from anywhere on the grid, shown inside the field and
 * declared as its `aria-keyshortcuts`. `/` because the tools this audience already lives in
 * spend it on search, so it is a key people try before they read anything.
 *
 * A constant here and not a `strings.ts` entry: it is a key, not copy — nothing translates it,
 * and `KeyboardEvent.key` reports `/` on a Turkish layout too, where it sits on Shift+7.
 */
const SEARCH_SHORTCUT = '/'

/**
 * The search box, and the chip that says what it is searching.
 *
 * A `<input type="search">`, so the clear affordance, Escape-to-clear and the right mobile
 * keyboard come from the browser rather than from code here.
 *
 * **Local state, debounced navigation.** The field responds to every keystroke and the URL
 * does not: `navigate` on each one would render the route per character and — without
 * `replace` — put every character in the back button's history. 250 ms is the pause after
 * typing, not a delay before feedback.
 *
 * **Two characters minimum, and it is not arbitrary.** A trigram is three characters, so
 * under that `gin_trgm_ops` cannot be used at all and the query is a sequential scan by
 * construction. The box accepts the keystroke and says it is waiting.
 *
 * ponytail: two characters because of the index, not because of the product. If a
 * one-character search is ever wanted, the fix is a prefix index, not removing this.
 */
export function SearchBox({
  q,
  categoryName,
  filtered,
  onSearch,
  onWiden,
}: {
  q: string
  categoryName: string | null
  filtered: boolean
  onSearch?: (query: string) => void
  onWiden: () => void
}) {
  const [typed, setTyped] = useState(q)
  // The URL is the source of truth: a back navigation or a shared link has to move the box,
  // and without this the field would keep whatever was last typed into it.
  const [lastFromUrl, setLastFromUrl] = useState(q)
  if (q !== lastFromUrl) {
    setLastFromUrl(q)
    setTyped(q)
  }

  useEffect(() => {
    const trimmed = typed.trim()
    // Below the minimum the query is not run — but an empty box *is* a change, because it
    // means "show me the library again".
    if (trimmed.length === 1) return
    if (trimmed === q) return
    const timer = setTimeout(() => onSearch?.(trimmed), 250)
    return () => clearTimeout(timer)
  }, [typed, q, onSearch])

  // `/` from anywhere on the grid. Not while a person is typing into a field — a slash is
  // half of every file path — and not under a modal dialog, whose own keys own the page.
  const field = useRef<HTMLInputElement>(null)
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== SEARCH_SHORTCUT || event.defaultPrevented) return
      if (event.ctrlKey || event.metaKey || event.altKey) return
      const target = event.target
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || target.closest('input, textarea, select') !== null)
      ) {
        return
      }
      if (document.querySelector('[aria-modal="true"]') !== null) return
      event.preventDefault()
      field.current?.focus()
      field.current?.select()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  const waiting = typed.trim().length === 1
  return (
    <div className="flex min-w-64 flex-1 flex-wrap items-center gap-2">
      {/*
        The field sits *below* the ground rather than on it — `--color-raised` against the
        page, which is how `v2` draws every input. A control you type into reads as a well;
        one you press reads as a surface.
      */}
      <div className="relative flex min-w-64 flex-1 items-center">
        <span
          aria-hidden="true"
          className="pointer-events-none absolute left-[11px] text-sm text-[var(--color-muted)]"
        >
          ⌕
        </span>
        <input
          ref={field}
          type="search"
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            // Back to the grid, keeping the query. Chrome's own Escape clears a search field,
            // which would throw away what was typed on the way out.
            event.preventDefault()
            document.getElementById('parts')?.focus()
          }}
          aria-label={strings.search.label}
          aria-keyshortcuts={SEARCH_SHORTCUT}
          placeholder={strings.search.placeholder}
          className="peer w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] py-2 pr-9 pl-[30px] text-[13px] focus:border-[var(--color-accent)]"
        />
        {/*
          The key, shown where it is used. Only while the field is empty and unfocused: once
          you are in it the hint has done its job, and over typed text it would be noise.
        */}
        {typed === '' ? (
          <kbd
            aria-hidden="true"
            className="ease-mechanical pointer-events-none absolute right-2.5 rounded border border-[var(--color-edge)] px-1.5 font-mono text-[11px] leading-4 text-[var(--color-muted)] duration-[var(--duration-fast)] peer-focus:opacity-0"
          >
            {SEARCH_SHORTCUT}
          </kbd>
        ) : null}
      </div>
      {/*
        The disclosure, not a control that narrows. The sidebar has already narrowed the
        grid; a search that quietly kept that narrowing without saying so is how somebody
        concludes a part is missing from the library. Dismissing it widens and keeps the
        query.
      */}
      {!filtered || q === '' ? null : (
        <button
          type="button"
          onClick={onWiden}
          title={strings.search.widen}
          className="ease-mechanical rounded-full border border-[var(--color-accent)] bg-[var(--color-surface)] px-3 py-1 text-xs text-[var(--color-text)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {categoryName === null
            ? strings.search.inThisCategory
            : strings.search.inCategory(categoryName)}{' '}
          {strings.glyphs.remove}
        </button>
      )}
      {!waiting ? null : (
        <p className="text-xs text-[var(--color-muted)]">{strings.search.keepTyping}</p>
      )}
    </div>
  )
}
