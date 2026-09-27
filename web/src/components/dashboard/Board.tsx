import { useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react'
import { flipFrom } from '../../lib/flip'
import { strings } from '../../lib/strings'
import type { StoredLayout, StoredWidget } from '../../lib/dashboard'
import type { WidgetResult } from '../../lib/types'
import { Dialog } from '../Dialog'
import { Icon } from '../Icon'
import { Menu } from '../Menu'
import { closeMenu } from '../Dialog'
import { WidgetSettings } from './AddWidget'
import { drawValue, limitsOf, registry, widgetTitle } from './registry'
import {
  COLUMNS,
  columnAt,
  inOrder,
  moveHorizontal,
  moveVertical,
  placeAt,
  resizeBy,
  rowAt,
  settle,
} from './layout'

/**
 * The board: named groups of widgets on a twelve-column grid, moved and resized by hand.
 *
 * **Hand-rolled, and `phase-6.md` says why.** react-grid-layout animates with CSS transitions
 * on `top`/`left`, which is two rules of this design system broken at once — layout properties,
 * and a duration nobody here chose — and WCAG needs the keyboard support we would have written
 * beside it regardless. So: pointer events with capture for the drag, arrow keys to move and
 * Shift+arrow to resize, an `aria-live` region that says where a widget went, and every
 * position computed by `layout.ts`, which is pure and tested on its own.
 *
 * **One row on a narrow screen.** The twelve columns and the row height are `md:` classes, so
 * under 768px the grid is `grid-cols-1` and every widget takes a row in the order the board is
 * in. That is CSS and not `matchMedia`, for the reason `AppFrame`'s drawer is: whether a widget
 * is on the page must not depend on a query the test renderer answers however it is stubbed.
 *
 * Nothing here fetches, and nothing here has a timer. Every value came out of the page's one
 * resolve; moving a widget costs no request at all.
 */

/** One grid row, in pixels, and the gap between them. The CSS reads these, so there is one copy. */
const ROW_PX = 68
const GAP_PX = 12

/** What a pointer drag divides an offset by to get a row. */
const ROW_PITCH = ROW_PX + GAP_PX

const PANEL =
  'relative flex min-h-0 flex-col overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]'

const HANDLE =
  'ease-mechanical flex size-6 flex-none touch-none cursor-grab items-center justify-center rounded-[3px] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] active:cursor-grabbing'

/**
 * The corner, drawn as two borders rather than a glyph.
 *
 * A real button in the tab order, and its arrow keys resize without Shift — the grip's
 * Shift+arrow is the same action from the header, and a corner nobody can reach by keyboard is
 * the half of a hand-rolled resize that WCAG is about.
 */
const CORNER =
  'absolute right-0 bottom-0 size-4 cursor-se-resize touch-none rounded-br-md border-r-2 border-b-2 border-[var(--color-edge)] opacity-60 hover:opacity-100'

const ITEM =
  'ease-mechanical w-full rounded-[3px] px-2 py-1.5 text-left text-xs text-[var(--color-text)] duration-[var(--duration-fast)] hover:bg-[var(--color-raised)]'

/** The widgets of one group, in reading order and settled — the group's own board. */
function tilesOf(layout: StoredLayout, group: string): StoredWidget[] {
  return settle(inOrder(layout.widgets.filter((widget) => widget.group === group)))
}

/** Put one group's settled widgets back, leaving every other group alone. */
function withGroup(layout: StoredLayout, group: string, tiles: readonly StoredWidget[]): StoredLayout {
  return { ...layout, widgets: [...layout.widgets.filter((widget) => widget.group !== group), ...tiles] }
}

/**
 * Every group settled, and any widget whose group has gone dropped.
 *
 * Used after a group is removed or a widget crosses between two, where both boards change. A
 * group is removed by first moving its widgets somewhere else, never by this.
 */
export function settleAll(layout: StoredLayout): StoredLayout {
  return { ...layout, widgets: layout.groups.flatMap((group) => tilesOf(layout, group.id)) }
}

function tileAt(tiles: readonly StoredWidget[], key: string): StoredWidget | undefined {
  return tiles.find((tile) => tile.key === key)
}

export function Board({
  layout,
  onLayout,
  results,
  onRetry,
}: {
  layout: StoredLayout
  onLayout: (next: StoredLayout) => void
  /** Each key's result from the page's one resolve; absent while it is in flight. */
  results: Map<string, WidgetResult>
  /** Ask for one key again — a person's press, never a timer. */
  onRetry: (key: string) => void
}) {
  /** What the live region last said. Where a widget went, in words. */
  const [message, setMessage] = useState('')
  const [settingsFor, setSettingsFor] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [removing, setRemoving] = useState<string | null>(null)

  /**
   * Every widget's element and the rectangle it occupied at the last commit.
   *
   * The FLIP is measured across commits rather than handed a rectangle by the drag, because a
   * widget moves for four different reasons — a drag, an arrow key, another widget resizing
   * under it, one being removed above it — and all four are the same movement to look at.
   */
  const nodes = useRef(new Map<string, HTMLElement>())
  const rects = useRef(new Map<string, DOMRect>())
  const grids = useRef(new Map<string, HTMLElement>())
  const handles = useRef(new Map<string, HTMLElement>())
  /** The cell a drag last applied, so a pointer moving inside one cell costs nothing. */
  const dragging = useRef<{ key: string; x: number; row: number } | null>(null)
  const resizing = useRef<{ key: string; w: number; h: number } | null>(null)
  /**
   * The widget a key just moved, so its handle can be given focus back.
   *
   * A move down reorders the array, and React reconciles that by relocating the widget's DOM
   * node — which blurs whatever had focus inside it. A move *up* relocates the other node, so
   * focus survives, and the bug would have been intermittent in exactly the way nobody
   * reproduces. `keyDown` in a test fires on an element reference and passes either way, so this
   * is held by the browser pass and by refocusing rather than by hoping.
   */
  const refocus = useRef<string | null>(null)

  useLayoutEffect(() => {
    for (const [key, node] of nodes.current) {
      const before = rects.current.get(key)
      const now = node.getBoundingClientRect()
      // 180ms and `transform` only — a widget changing places is a state change, which is what
      // `--duration-base` is for. `flipFrom` declines under reduced motion and where there is
      // nothing to animate, which is every test runner and every reader who asked for less.
      if (before !== undefined) flipFrom(node, before, 'base')
      rects.current.set(key, now)
    }
    for (const key of [...rects.current.keys()]) {
      if (!nodes.current.has(key)) rects.current.delete(key)
    }
    const wanted = refocus.current
    if (wanted !== null) {
      refocus.current = null
      handles.current.get(wanted)?.focus()
    }
  })

  const apply = (group: string, tiles: readonly StoredWidget[], key: string, announce: 'moved' | 'resized') => {
    const next = withGroup(layout, group, tiles)
    onLayout(next)
    const tile = tileAt(tiles, key)
    const name = layout.groups.find((entry) => entry.id === group)?.name ?? ''
    if (tile === undefined) return
    const title = widgetTitle(tile, results.get(key))
    setMessage(
      announce === 'moved'
        ? strings.dashboard.moved(title, name, tile.x + 1, tile.y + 1)
        : strings.dashboard.resized(title, tile.w, tile.h),
    )
  }

  const onKeyDown = (event: React.KeyboardEvent, stored: StoredWidget, corner = false) => {
    const dx = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0
    const dy = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0
    if (dx === 0 && dy === 0) return
    event.preventDefault()
    const tiles = tilesOf(layout, stored.group)
    if (event.shiftKey || corner) {
      // Shift+arrow resizes: right and down grow, left and up shrink, inside the kind's limits.
      apply(stored.group, resizeBy(tiles, stored.key, dx, dy, limitsOf(stored.widget)), stored.key, 'resized')
      return
    }
    const moved = dx !== 0 ? moveHorizontal(tiles, stored.key, dx) : moveVertical(tiles, stored.key, dy)
    // Asked for before the layout changes, because reordering relocates this widget's node.
    refocus.current = corner ? null : stored.key
    apply(stored.group, moved, stored.key, 'moved')
  }

  /**
   * Take the pointer, so the drag keeps going when it leaves the handle.
   *
   * jsdom implements no pointer capture, and neither does a browser driving this with a keyboard.
   * Guarded for the reason `flipFrom` guards `animate`: a drag that throws on pointerdown is
   * worse than one that follows the pointer without capture.
   */
  const capture = (event: ReactPointerEvent<HTMLElement>) => {
    const handle = event.currentTarget
    if (typeof handle.setPointerCapture === 'function') handle.setPointerCapture(event.pointerId)
  }

  /** Where in the group's grid a pointer is, in cells. */
  const cellUnder = (event: ReactPointerEvent<HTMLElement>, group: string) => {
    const grid = grids.current.get(group)
    if (grid === undefined) return null
    const box = grid.getBoundingClientRect()
    return {
      x: columnAt(event.clientX - box.left, box.width),
      row: rowAt(event.clientY - box.top, ROW_PITCH),
    }
  }

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => {
    // Primary button only.
    if (event.button !== 0) return
    capture(event)
    dragging.current = { key: stored.key, x: stored.x, row: stored.y }
  }

  const onPointerMove = (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => {
    const drag = dragging.current
    if (drag === null || drag.key !== stored.key) return
    const cell = cellUnder(event, stored.group)
    if (cell === null || (cell.x === drag.x && cell.row === drag.row)) return
    dragging.current = { key: stored.key, x: cell.x, row: cell.row }
    apply(stored.group, placeAt(tilesOf(layout, stored.group), stored.key, cell.x, cell.row), stored.key, 'moved')
  }

  const onResizeDown = (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => {
    if (event.button !== 0) return
    capture(event)
    resizing.current = { key: stored.key, w: stored.w, h: stored.h }
  }

  /**
   * Drag the corner: the cell under the pointer becomes the widget's far corner.
   *
   * Expressed as a delta and handed to `resizeBy`, so the kind's limits and the right edge hold
   * for a pointer exactly as they do for Shift+arrow — one definition of how large a widget may
   * be, in the pure module.
   */
  const onResizeMove = (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => {
    const drag = resizing.current
    if (drag === null || drag.key !== stored.key) return
    const cell = cellUnder(event, stored.group)
    if (cell === null) return
    const w = Math.max(1, cell.x - stored.x + 1)
    const h = Math.max(1, cell.row - stored.y + 1)
    if (w === drag.w && h === drag.h) return
    resizing.current = { key: stored.key, w, h }
    const tiles = tilesOf(layout, stored.group)
    apply(
      stored.group,
      resizeBy(tiles, stored.key, w - stored.w, h - stored.h, limitsOf(stored.widget)),
      stored.key,
      'resized',
    )
  }

  const endDrag = (event: ReactPointerEvent<HTMLElement>) => {
    dragging.current = null
    resizing.current = null
    const handle = event.currentTarget
    if (typeof handle.releasePointerCapture === 'function' && handle.hasPointerCapture?.(event.pointerId)) {
      handle.releasePointerCapture(event.pointerId)
    }
  }

  const changeGroup = (stored: StoredWidget, group: string) => {
    const moved = layout.widgets.map((widget) =>
      widget.key === stored.key ? { ...widget, group, x: 0, y: Number.MAX_SAFE_INTEGER } : widget,
    )
    const next = settleAll({ ...layout, widgets: moved })
    // The panel is unmounted from one section and mounted in another, so focus has to be asked
    // for again by name — there is no node left to keep it.
    refocus.current = stored.key
    onLayout(next)
    const tile = next.widgets.find((widget) => widget.key === stored.key)
    const name = layout.groups.find((entry) => entry.id === group)?.name ?? ''
    if (tile !== undefined) {
      setMessage(strings.dashboard.moved(widgetTitle(tile, results.get(stored.key)), name, tile.x + 1, tile.y + 1))
    }
  }

  const removeWidget = (key: string) => {
    onLayout(settleAll({ ...layout, widgets: layout.widgets.filter((widget) => widget.key !== key) }))
  }

  const reorderGroup = (index: number, by: number) => {
    const to = index + by
    const groups = [...layout.groups]
    const group = groups[index]
    const other = groups[to]
    if (group === undefined || other === undefined) return
    groups[index] = other
    groups[to] = group
    onLayout({ ...layout, groups })
  }

  const removeGroup = (id: string) => {
    const remaining = layout.groups.filter((group) => group.id !== id)
    const home = remaining[0]?.id
    if (home === undefined) return
    // The widgets move first, and then the group goes. Nothing on this page deletes anything —
    // a group is a heading, and its widgets are panels that have to be somewhere.
    const widgets = layout.widgets.map((widget) =>
      widget.group === id ? { ...widget, group: home, x: 0, y: Number.MAX_SAFE_INTEGER } : widget,
    )
    onLayout(settleAll({ ...layout, groups: remaining, widgets }))
    setRemoving(null)
  }

  const renameGroup = (id: string, name: string) => {
    onLayout({
      ...layout,
      groups: layout.groups.map((group) => (group.id === id ? { ...group, name } : group)),
    })
    setRenaming(null)
  }

  const settingsWidget = layout.widgets.find((widget) => widget.key === settingsFor)
  const renamingGroup = layout.groups.find((group) => group.id === renaming)
  const removingGroup = layout.groups.find((group) => group.id === removing)
  const home = layout.groups.find((group) => group.id !== removing)

  return (
    <>
      {/*
        Where a widget went, for somebody who cannot see it move. Labelled, so a test driver and
        a screen reader can both address it by name — the grid page already holds four unnamed
        `status` regions and `getByRole('status')` there is ambiguous.
      */}
      <div role="status" aria-label={strings.dashboard.positions} className="sr-only">
        {message}
      </div>
      {layout.groups.map((group, index) => {
        const tiles = tilesOf(layout, group.id)
        return (
          <section key={group.id} className="mt-8 first:mt-6" aria-labelledby={`dashboard-group-${group.id}`}>
            <div className="flex items-center gap-2 border-b border-[var(--color-border)] pb-2">
              <h3
                id={`dashboard-group-${group.id}`}
                className="grow text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase"
              >
                {group.name}
              </h3>
              <Menu id={`dashboard-group-menu-${group.id}`} label={strings.dashboard.groupMenu(group.name)} icon="more">
                <button type="button" className={ITEM} onClick={(event) => { closeMenu(event.currentTarget); setRenaming(group.id) }}>
                  {strings.dashboard.renameGroup(group.name)}
                </button>
                <button
                  type="button"
                  className={ITEM}
                  disabled={index === 0}
                  onClick={(event) => { closeMenu(event.currentTarget); reorderGroup(index, -1) }}
                >
                  {strings.dashboard.groupUp(group.name)}
                </button>
                <button
                  type="button"
                  className={ITEM}
                  disabled={index === layout.groups.length - 1}
                  onClick={(event) => { closeMenu(event.currentTarget); reorderGroup(index, 1) }}
                >
                  {strings.dashboard.groupDown(group.name)}
                </button>
                <button
                  type="button"
                  className={ITEM}
                  disabled={layout.groups.length === 1}
                  title={layout.groups.length === 1 ? strings.dashboard.lastGroup : undefined}
                  onClick={(event) => { closeMenu(event.currentTarget); setRemoving(group.id) }}
                >
                  {strings.dashboard.removeGroup(group.name)}
                </button>
              </Menu>
            </div>
            <div
              ref={(node) => {
                if (node === null) grids.current.delete(group.id)
                else grids.current.set(group.id, node)
              }}
              style={{ '--dash-row': `${ROW_PX}px`, gap: `${GAP_PX}px` } as CSSProperties}
              className="mt-3 grid grid-cols-1 md:[grid-auto-rows:var(--dash-row)] md:[grid-template-columns:repeat(12,minmax(0,1fr))]"
            >
              {tiles.map((stored) => (
                <Panel
                  key={stored.key}
                  stored={stored}
                  groups={layout.groups}
                  result={results.get(stored.key)}
                  onRetry={onRetry}
                  onKeyDown={onKeyDown}
                  onPointerDown={onPointerDown}
                  onPointerMove={onPointerMove}
                  onResizeDown={onResizeDown}
                  onResizeMove={onResizeMove}
                  onPointerUp={endDrag}
                  onSettings={setSettingsFor}
                  onChangeGroup={changeGroup}
                  onRemove={removeWidget}
                  register={(node) => {
                    if (node === null) nodes.current.delete(stored.key)
                    else nodes.current.set(stored.key, node)
                  }}
                  registerHandle={(node) => {
                    if (node === null) handles.current.delete(stored.key)
                    else handles.current.set(stored.key, node)
                  }}
                />
              ))}
            </div>
          </section>
        )
      })}
      {settingsWidget === undefined ? null : (
        <WidgetSettings
          stored={settingsWidget}
          onClose={() => setSettingsFor(null)}
          onSave={(widget) => {
            onLayout({
              ...layout,
              widgets: layout.widgets.map((entry) =>
                entry.key === settingsWidget.key ? { ...entry, widget } : entry,
              ),
            })
            setSettingsFor(null)
          }}
        />
      )}
      {renamingGroup === undefined ? null : (
        <RenameGroup group={renamingGroup} onClose={() => setRenaming(null)} onRename={renameGroup} />
      )}
      {removingGroup === undefined || home === undefined ? null : (
        <Dialog title={strings.dashboard.removeGroup(removingGroup.name)} onClose={() => setRemoving(null)}>
          <p className="max-w-prose text-sm text-[var(--color-dim)]">
            {strings.dashboard.removeGroupKeeps(removingGroup.name, home.name)}
          </p>
          <button
            type="button"
            className="ease-mechanical mt-4 self-start rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
            onClick={() => removeGroup(removingGroup.id)}
          >
            {strings.dashboard.removeGroupConfirm}
          </button>
        </Dialog>
      )}
    </>
  )
}

function RenameGroup({
  group,
  onClose,
  onRename,
}: {
  group: { id: string; name: string }
  onClose: () => void
  onRename: (id: string, name: string) => void
}) {
  const [name, setName] = useState(group.name)
  return (
    <Dialog title={strings.dashboard.renameGroupTitle(group.name)} onClose={onClose}>
      <label className="mt-2 flex flex-col gap-1 text-xs text-[var(--color-muted)]">
        {strings.dashboard.groupNameLabel}
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-2 py-1.5 text-sm text-[var(--color-text)]"
        />
      </label>
      <button
        type="button"
        disabled={name.trim().length === 0}
        className="ease-mechanical mt-4 self-start rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        onClick={() => onRename(group.id, name.trim())}
      >
        {strings.dashboard.renameGroupConfirm}
      </button>
    </Dialog>
  )
}

/**
 * One widget: a header it is moved by, whatever the resolve said, and a corner it is resized by.
 *
 * The arrow keys are on the **header's grip**, not on the panel, because the panel holds links
 * and a keypress inside it belongs to whatever has focus there. The grip is one tab stop, keeps
 * focus across a move — the element is the same element wherever the grid puts it — and carries
 * the hint that says what the keys do.
 */
function Panel({
  stored,
  groups,
  result,
  onRetry,
  onKeyDown,
  onPointerDown,
  onPointerMove,
  onResizeDown,
  onResizeMove,
  onPointerUp,
  onSettings,
  onChangeGroup,
  onRemove,
  register,
  registerHandle,
}: {
  stored: StoredWidget
  groups: readonly { id: string; name: string }[]
  result: WidgetResult | undefined
  onRetry: (key: string) => void
  onKeyDown: (event: React.KeyboardEvent, stored: StoredWidget, corner?: boolean) => void
  onPointerDown: (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => void
  onPointerMove: (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => void
  onResizeDown: (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => void
  onResizeMove: (event: ReactPointerEvent<HTMLElement>, stored: StoredWidget) => void
  onPointerUp: (event: ReactPointerEvent<HTMLElement>) => void
  onSettings: (key: string) => void
  onChangeGroup: (stored: StoredWidget, group: string) => void
  onRemove: (key: string) => void
  register: (node: HTMLElement | null) => void
  registerHandle: (node: HTMLElement | null) => void
}) {
  const title = widgetTitle(stored, result)
  const spec = registry[stored.widget.kind]
  return (
    <article
      ref={register}
      aria-labelledby={`dashboard-widget-${stored.key}`}
      style={
        {
          '--dash-col': `${stored.x + 1} / span ${Math.min(stored.w, COLUMNS)}`,
          '--dash-span': `${stored.y + 1} / span ${stored.h}`,
        } as CSSProperties
      }
      className={`${PANEL} md:[grid-column:var(--dash-col)] md:[grid-row:var(--dash-span)]`}
    >
      <header className="flex flex-none items-center gap-1.5 border-b border-[var(--color-border)] px-2 py-1.5">
        <button
          ref={registerHandle}
          type="button"
          aria-label={strings.dashboard.moveLabel(title)}
          title={strings.dashboard.moveHint}
          className={HANDLE}
          onKeyDown={(event) => onKeyDown(event, stored)}
          onPointerDown={(event) => onPointerDown(event, stored)}
          onPointerMove={(event) => onPointerMove(event, stored)}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          <Icon name="list" size={14} />
        </button>
        <h4
          id={`dashboard-widget-${stored.key}`}
          className="min-w-0 grow truncate text-xs font-medium text-[var(--color-bright)]"
        >
          {title}
        </h4>
        <Menu id={`dashboard-widget-menu-${stored.key}`} label={strings.dashboard.widgetMenu(title)} icon="more">
          {spec.fields.length === 0 ? null : (
            <button type="button" className={ITEM} onClick={(event) => { closeMenu(event.currentTarget); onSettings(stored.key) }}>
              {strings.dashboard.settings}
            </button>
          )}
          {groups
            .filter((group) => group.id !== stored.group)
            .map((group) => (
              <button
                key={group.id}
                type="button"
                className={ITEM}
                onClick={(event) => { closeMenu(event.currentTarget); onChangeGroup(stored, group.id) }}
              >
                {strings.dashboard.moveToGroup(group.name)}
              </button>
            ))}
          <button
            type="button"
            className={ITEM}
            title={strings.dashboard.removeWidgetDetail}
            onClick={(event) => { closeMenu(event.currentTarget); onRemove(stored.key) }}
          >
            {strings.dashboard.removeWidget}
          </button>
        </Menu>
      </header>
      <div className="min-h-0 grow overflow-y-auto px-2.5 py-2">
        <Body stored={stored} result={result} onRetry={onRetry} />
      </div>
      <button
        type="button"
        aria-label={strings.dashboard.resizeLabel(title)}
        className={CORNER}
        onKeyDown={(event) => onKeyDown(event, stored, true)}
        onPointerDown={(event) => onResizeDown(event, stored)}
        onPointerMove={(event) => onResizeMove(event, stored)}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      />
    </article>
  )
}

/**
 * What one key came back as.
 *
 * Three answers, and each of them is this widget's alone: `phase-6.md`'s resolve always answers
 * 200 once the body is valid, so a key that ran out of time or failed says so **while every
 * other widget on the page renders**. The retry asks for this one key again, which is one more
 * resolve on a press — not a timer, and not a per-widget endpoint, neither of which exists.
 */
function Body({
  stored,
  result,
  onRetry,
}: {
  stored: StoredWidget
  result: WidgetResult | undefined
  onRetry: (key: string) => void
}) {
  if (result === undefined) {
    return <p className="text-xs text-[var(--color-muted)]">{strings.dashboard.loading}</p>
  }
  if (result.status === 'ok') {
    return <>{drawValue(stored.widget, result.value)}</>
  }
  const title = widgetTitle(stored, result)
  // Both read outside the JSX: the bare-strings gate reports every literal in a child
  // expression, a `status` comparison included, and narrowing here also gives `result.message`
  // to TypeScript without a second check.
  const ranOut = result.status === 'timedOut'
  const because = result.status === 'failed' ? result.message : null
  return (
    <div className="flex flex-col items-start gap-2">
      <p className="text-xs text-[var(--color-muted)]">
        {ranOut ? strings.dashboard.timedOut : strings.dashboard.widgetFailed}
      </p>
      {because === null ? null : <p className="text-xs text-[var(--color-dim)]">{because}</p>}
      <button
        type="button"
        aria-label={strings.dashboard.retryLabel(title)}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-2.5 py-1 text-xs text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
        onClick={() => onRetry(stored.key)}
      >
        {strings.dashboard.retry}
      </button>
    </div>
  )
}
