import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { fetchLibraries, fetchSavedFilters } from '../../lib/api'
import { MAX_LIMIT, MAX_WIDGETS, nextKey, widgetFrom } from '../../lib/dashboard'
import type { StoredLayout, StoredWidget } from '../../lib/dashboard'
import { strings } from '../../lib/strings'
import type { FacetKind, LibraryId, SavedFilterId, Widget } from '../../lib/types'
import { Dialog } from '../Dialog'
import { KINDS, registry, type Field } from './registry'

/**
 * Adding a widget, changing one's settings, and adding a group.
 *
 * **Every kind's form is the same four controls**, and which of them a kind offers is declared in
 * the registry rather than drawn seven times. That is the whole reason `WidgetSpec.fields` exists:
 * a form component per kind would put "which library" in seven places, and the eighth kind added
 * in Rust would ship with a form somebody forgot to write instead of a `tsc` error.
 *
 * **Nothing here reads anything until a dialog is open.** The library list and a library's saved
 * filters are two requests the dashboard does not make: the page costs one resolve, and these
 * queries live inside components that exist only while somebody is filling the form in. A
 * `useQuery` mounted beside the board would make the page's one request three.
 */

const FIELD = 'mt-3 flex flex-col gap-1 text-xs text-[var(--color-muted)]'

const INPUT =
  'rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-2 py-1.5 text-sm text-[var(--color-text)]'

const CONFIRM =
  'ease-mechanical mt-5 self-start rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

const OPENER =
  'ease-mechanical flex min-h-8 flex-none items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-xs text-[var(--color-text)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/** What the form is holding, before it is a `Widget`. Loose on purpose: {@link widgetFrom} judges it. */
type Draft = {
  kind: Widget['kind']
  library: LibraryId
  filter: SavedFilterId | ''
  facet: FacetKind
  limit: number
}

const FACETS: readonly FacetKind[] = ['format', 'material', 'tag']

/**
 * Which of the four controls a kind asks for, as four booleans.
 *
 * Read out here and not inside the JSX: the bare-strings gate reports every literal written in a
 * child expression, `fields.includes('library')` included, and it is right to — a formatter
 * inlined the same way is how a singular branch gets dropped.
 */
function asks(fields: readonly Field[]) {
  return {
    library: fields.includes('library'),
    filter: fields.includes('filter'),
    facet: fields.includes('facet'),
    limit: fields.includes('limit'),
  }
}

function facetLabel(facet: FacetKind): string {
  return facet === 'format' ? strings.facets.format : facet === 'material' ? strings.facets.material : strings.facets.tag
}

/**
 * The controls one kind needs, and no others.
 *
 * The saved filter list is read only when a kind asks for one, so choosing `Recently added`
 * costs nothing. A library with no saved filters says so rather than offering an empty select —
 * the widget would resolve to a key that fails, which is a worse way to learn the same thing.
 */
function Config({
  draft,
  onChange,
  libraries,
}: {
  draft: Draft
  onChange: (draft: Draft) => void
  libraries: readonly { id: LibraryId; name: string }[]
}) {
  const wants = asks(registry[draft.kind].fields)
  const filters = useQuery({
    queryKey: ['savedFilters', draft.library],
    queryFn: () => fetchSavedFilters(draft.library),
    enabled: wants.filter,
  })
  const saved = filters.data ?? []
  return (
    <>
      {!wants.library ? null : (
        <label className={FIELD}>
          {strings.dashboard.addLibrary}
          <select
            className={INPUT}
            value={draft.library}
            onChange={(event) => onChange({ ...draft, library: event.target.value, filter: '' })}
          >
            {libraries.map((library) => (
              <option key={library.id} value={library.id}>
                {library.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {!wants.filter ? null : saved.length === 0 ? (
        <p className="mt-3 max-w-prose text-xs text-[var(--color-muted)]">{strings.dashboard.noFilters}</p>
      ) : (
        <label className={FIELD}>
          {strings.dashboard.addFilter}
          <select
            className={INPUT}
            value={draft.filter}
            onChange={(event) => onChange({ ...draft, filter: event.target.value })}
          >
            <option value="" />
            {saved.map((filter) => (
              <option key={filter.id} value={filter.id}>
                {filter.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {!wants.facet ? null : (
        <label className={FIELD}>
          {strings.dashboard.addFacet}
          <select
            className={INPUT}
            value={draft.facet}
            onChange={(event) => onChange({ ...draft, facet: event.target.value as FacetKind })}
          >
            {FACETS.map((facet) => (
              <option key={facet} value={facet}>
                {facetLabel(facet)}
              </option>
            ))}
          </select>
        </label>
      )}
      {!wants.limit ? null : (
        <label className={FIELD}>
          {strings.dashboard.addLimit}
          {/*
            A number input, not a select of twelve: the ceiling is the route's and the browser
            already knows how to keep a number inside one. `widgetFrom` clamps it regardless,
            because a typed value is not a chosen one.
          */}
          <input
            type="number"
            min={1}
            max={MAX_LIMIT}
            className={INPUT}
            value={draft.limit}
            onChange={(event) => onChange({ ...draft, limit: Number(event.target.value) })}
          />
        </label>
      )}
    </>
  )
}

/** The library list, read once a dialog needs it and shared by both of them through the cache. */
function useLibraries(fallback: LibraryId) {
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  const known = libraries.data ?? []
  return known.length > 0 ? known : [{ id: fallback, name: fallback }]
}

/**
 * Add a widget to the dashboard.
 *
 * Refused at {@link MAX_WIDGETS}, and the button says why rather than going quiet: the resolve
 * route refuses a body of more than 32 keys **whole**, with a 422, so the thirty-third widget
 * would not fail — the whole dashboard would.
 */
export function AddWidget({
  layout,
  onLayout,
  library,
  taken,
}: {
  layout: StoredLayout
  onLayout: (next: StoredLayout) => void
  /** The library the page is about, as the form's first offer. */
  library: LibraryId
  /**
   * Every key already spoken for — on the board *and* in the answer still cached. Wider than the
   * layout on purpose: a key reused after its widget was removed would draw that widget's cached
   * result under the new one's heading.
   */
  taken: readonly string[]
}) {
  const [open, setOpen] = useState(false)
  const full = layout.widgets.length >= MAX_WIDGETS
  return (
    <>
      <button
        type="button"
        className={OPENER}
        disabled={full}
        title={full ? strings.dashboard.full(MAX_WIDGETS) : undefined}
        onClick={() => setOpen(true)}
      >
        {strings.dashboard.add}
      </button>
      {!open ? null : (
        <AddWidgetDialog
          layout={layout}
          library={library}
          onClose={() => setOpen(false)}
          onAdd={(widget, group, libraryName) => {
            const spec = registry[widget.kind]
            const stored: StoredWidget = {
              key: nextKey(taken, 'w'),
              widget,
              group,
              x: 0,
              // Last, wherever that is. The board settles every group as it draws it, and the
              // route normalises what is written, so no caller works out a row.
              y: Number.MAX_SAFE_INTEGER,
              w: spec.size.w,
              h: spec.size.h,
              libraryName,
            }
            onLayout({ ...layout, widgets: [...layout.widgets, stored] })
            setOpen(false)
          }}
        />
      )}
    </>
  )
}

function AddWidgetDialog({
  layout,
  library,
  onClose,
  onAdd,
}: {
  layout: StoredLayout
  library: LibraryId
  onClose: () => void
  onAdd: (widget: Widget, group: string, libraryName: string | null) => void
}) {
  const libraries = useLibraries(library)
  const [group, setGroup] = useState(layout.groups[0]?.id ?? '')
  const [draft, setDraft] = useState<Draft>({
    kind: 'storage',
    library,
    filter: '',
    facet: 'format',
    limit: MAX_LIMIT,
  })
  const widget = widgetFrom(draft)
  return (
    <Dialog title={strings.dashboard.addTitle} onClose={onClose}>
      <label className={FIELD}>
        {strings.dashboard.addKind}
        <select
          className={INPUT}
          value={draft.kind}
          onChange={(event) => setDraft({ ...draft, kind: event.target.value as Widget['kind'] })}
        >
          {KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {registry[kind].label}
            </option>
          ))}
        </select>
      </label>
      <Config draft={draft} onChange={setDraft} libraries={libraries} />
      {layout.groups.length < 2 ? null : (
        <label className={FIELD}>
          {strings.dashboard.addGroupField}
          <select className={INPUT} value={group} onChange={(event) => setGroup(event.target.value)}>
            {layout.groups.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.name}
              </option>
            ))}
          </select>
        </label>
      )}
      <button
        type="button"
        className={CONFIRM}
        disabled={widget === null}
        onClick={() => {
          if (widget === null) return
          // The library's name as it is now, stored beside the widget: no widget's value names
          // its library, so two of these side by side would otherwise be the same panel twice.
          const named = libraries.find((entry) => entry.id === draft.library)
          onAdd(widget, group, widget.kind === 'instanceStorage' ? null : (named?.name ?? null))
        }}
      >
        {strings.dashboard.addConfirm}
      </button>
    </Dialog>
  )
}

/** One widget's settings, afterwards: the same controls, filled in from what it is showing now. */
export function WidgetSettings({
  stored,
  onClose,
  onSave,
}: {
  stored: StoredWidget
  onClose: () => void
  onSave: (widget: Widget) => void
}) {
  const configured = stored.widget
  const libraryOf = configured.kind === 'instanceStorage' ? '' : configured.library
  const libraries = useLibraries(libraryOf)
  const [draft, setDraft] = useState<Draft>({
    kind: configured.kind,
    library: libraryOf,
    filter: configured.kind === 'savedFilter' ? configured.filter : '',
    facet: configured.kind === 'facet' ? configured.facet : 'format',
    limit:
      configured.kind === 'recent' || configured.kind === 'savedFilter' || configured.kind === 'facet'
        ? configured.limit
        : MAX_LIMIT,
  })
  const widget = widgetFrom(draft)
  return (
    <Dialog title={strings.dashboard.settingsTitle(registry[stored.widget.kind].label)} onClose={onClose}>
      <Config draft={draft} onChange={setDraft} libraries={libraries} />
      <button type="button" className={CONFIRM} disabled={widget === null} onClick={() => widget !== null && onSave(widget)}>
        {strings.dashboard.settingsSave}
      </button>
    </Dialog>
  )
}

/** A new named group, under the last one. Widgets move into it from their own menus. */
export function AddGroup({
  layout,
  onLayout,
}: {
  layout: StoredLayout
  onLayout: (next: StoredLayout) => void
}) {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const suggested = strings.dashboard.newGroupName(layout.groups.length + 1)
  return (
    <>
      <button
        type="button"
        className={`${OPENER} mt-6`}
        onClick={() => {
          setName(suggested)
          setOpen(true)
        }}
      >
        {strings.dashboard.addGroup}
      </button>
      {!open ? null : (
        <Dialog title={strings.dashboard.addGroupTitle} onClose={() => setOpen(false)}>
          <label className={FIELD}>
            {strings.dashboard.groupNameLabel}
            <input className={INPUT} value={name} onChange={(event) => setName(event.target.value)} />
          </label>
          <button
            type="button"
            className={CONFIRM}
            disabled={name.trim().length === 0}
            onClick={() => {
              const id = nextKey(layout.groups.map((group) => group.id), 'g')
              onLayout({ ...layout, groups: [...layout.groups, { id, name: name.trim() }] })
              setOpen(false)
            }}
          >
            {strings.dashboard.addGroupConfirm}
          </button>
        </Dialog>
      )}
    </>
  )
}
