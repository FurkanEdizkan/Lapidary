/*
  `Menu` is a native popover, closed until its button is pressed. The preview opens it once on
  mount so the card shows the panel; the items are the Library menu's own, class for class, from
  `LibraryMenu` in `src/components/Toolbar.tsx`.
*/
import { Menu } from 'lapidary-web'
import { useEffect } from 'react'

const item =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/** The header's Library menu, open. */
export function Library() {
  useEffect(() => {
    document.getElementById('preview-library-menu')?.showPopover()
  }, [])
  return (
    <div style={{ padding: 16, minHeight: 360 }}>
      <Menu id="preview-library-menu" label="Library">
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" defaultChecked className="accent-[var(--color-accent)]" />
          Render previews automatically as parts are ingested
        </label>
        <button type="button" className={item}>
          Scan the ingest folder
        </button>
        <button type="button" className={item}>
          Generate missing previews
        </button>
        <button type="button" className={item}>
          Import a bundle
        </button>
      </Menu>
    </div>
  )
}

/** Closed: the quiet button a menu is at rest. */
export function Closed() {
  return (
    <div style={{ padding: 16 }}>
      <Menu id="preview-closed-menu" label="View">
        <button type="button" className={item}>
          Detail
        </button>
      </Menu>
    </div>
  )
}
