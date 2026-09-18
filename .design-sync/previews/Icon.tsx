/*
  Every glyph Lapidary vendors from Phosphor (regular weight), at 16px — where the line is one pixel —
  and at 24px, with the name you pass as `name`. Then the rule the component exists under: an icon is
  always `aria-hidden` and always inside a control that carries its own words, never the only label.
*/
import { Icon } from 'lapidary-web'
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

const NAMES = ['upload', 'search', 'list', 'more', 'caretDown', 'close'] as const

/** The six glyphs, Graphite, at 16px and 24px. */
export function Glyphs() {
  return (
    <Ground>
      <div style={{ display: 'flex', gap: 28, color: 'var(--color-muted)' }}>
        {NAMES.map((name) => (
          <div key={name} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10 }}>
            <div style={{ display: 'flex', alignItems: 'flex-end', gap: 10, color: 'var(--color-text)' }}>
              <Icon name={name} />
              <Icon name={name} size={24} />
            </div>
            <code className="tabular text-[11px]">{name}</code>
          </div>
        ))}
      </div>
    </Ground>
  )
}

/** In the header, as Upload uses it: the glyph beside the words, the words naming the control. */
export function InAControl() {
  return (
    <Ground>
      <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
        <button
          type="button"
          className="ease-mechanical inline-flex items-center gap-2 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-3 py-1.5 text-xs font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          <Icon name="upload" />
          Upload
        </button>
        <button
          type="button"
          aria-label="More actions for flange-dn40-lp-3310-02"
          className="ease-mechanical inline-flex items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1.5 text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          <Icon name="more" />
        </button>
      </div>
    </Ground>
  )
}
