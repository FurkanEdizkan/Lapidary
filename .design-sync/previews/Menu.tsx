/*
  `Menu` is a native popover, closed until its button is pressed. The preview opens it once on
  mount so the card shows the panel; the items are the Library menu's own, class for class, from
  `LibraryMenu` in `src/components/Toolbar.tsx`. The button sits at the right edge, where the header puts
  it: the panel is aligned to the button's right side and opens leftward.
*/
import { Menu } from 'lapidary-web'
import { useEffect } from 'react'

import type React from 'react'

/**
 * The app's page ground. Lapidary paints it on `:root`, and the preview page's own white `body`
 * covers that — so every preview lays its ground down itself, filling the cell, as the app's
 * screen would be.
 */
function Ground({ children, padded = true }: { children: React.ReactNode; padded?: boolean }) {
  return (
    <div
      style={{
        background: 'var(--color-bg)',
        color: 'var(--color-text)',
        minHeight: 'calc(100vh - 48px)',
        padding: padded ? 16 : 0,
      }}
    >
      {children}
    </div>
  )
}

const item =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/** The header's Library menu, open. */
export function Library() {
  useEffect(() => {
    document.getElementById('preview-library-menu')?.showPopover()
  }, [])
  return (
    <Ground>
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
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
    </Ground>
  )
}

/** Closed: the quiet button a menu is at rest. */
export function Closed() {
  return (
    <Ground>
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
      <Menu id="preview-closed-menu" label="View">
        <button type="button" className={item}>
          Detail
        </button>
      </Menu>
      </div>
    </Ground>
  )
}
