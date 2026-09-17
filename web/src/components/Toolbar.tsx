import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'
import { createLibrary, fetchLibraries, makeControlled } from '../lib/api'
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
 * The library's own menu, in the header: which library, and everything that acts on all of it.
 *
 * Switching, making a new one, the preview setting, a scan, a preview sweep, the field and
 * density definitions, and importing a bundle. These are set once and left, or done now and
 * then, so they sit behind one button rather than across the bar; what a person touches every
 * visit (search, Upload, the places) stays on it.
 *
 * Menus are native popovers (`styles.css` has why). Every control inside one stays in the
 * document while the menu is closed, which is what keeps a test that finds the auto-thumbnail
 * checkbox by role meaningful — and also what would let such a test pass if the menu could
 * never be opened at all, so `index.test.tsx` asserts the trigger is wired to its menu, and
 * the keyboard pass in the browser is the check that it opens.
 */
export function LibraryMenu({
  autoThumbnail,
  onAutoThumbnail,
  settingsBusy,
  onScan,
  scanBusy,
  onSweep,
  sweepBusy,
  library,
  onSelectLibrary,
  onImport,
  importBusy,
}: {
  autoThumbnail: boolean | undefined
  onAutoThumbnail: (on: boolean) => void
  settingsBusy: boolean
  onScan: () => void
  scanBusy: boolean
  onSweep: () => void
  sweepBusy: boolean
  library: LibraryId
  onSelectLibrary?: (library: LibraryId) => void
  /** Opens the picker for a bundle exported from Lapidary. */
  onImport: () => void
  importBusy: boolean
}) {
  return (
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
      <button
        type="button"
        onClick={onImport}
        disabled={importBusy}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {strings.toolbar.importBundle}
      </button>
    </Menu>
  )
}

/**
 * The row over the grid: what you are looking at on the left, and selection and the view menu
 * on the right, then the notes the header's menus leave.
 *
 * The places, search, the library menu and Upload moved to the header (`AppFrame`). What stays
 * is what is about *this grid*: its scope, whether you are selecting, and how the cards are
 * laid out.
 */
export function Toolbar({
  scope,
  settingsNote,
  note,
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
}: {
  /** The folder's name and how many parts are showing, built by the page that knows both. */
  scope: ReactNode
  settingsNote: string | null
  note: string | null
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
}) {
  return (
    <>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <div className="mr-auto flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">{scope}</div>
        <div className="flex flex-none items-center gap-2">
        <button
          type="button"
          aria-pressed={selecting}
          onClick={() => onSelecting(!selecting)}
          className="ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] aria-pressed:border-[var(--color-accent)] aria-pressed:text-[var(--color-bright)]"
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
        </div>
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
