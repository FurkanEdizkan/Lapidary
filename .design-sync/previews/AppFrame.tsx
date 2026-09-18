/*
  The one header, around the two kinds of page Lapidary has: the parts wall with its category
  rail, and a page past the grid laid out by `Page.ts` (one Headline, a lead, sections that open on
  a label over a scribe line, one standing control). The class strings below are `Page.ts`'s own,
  copied because that module's constants are not bundle exports.

  The rail is the real `FolderTree`, fed a seeded category list through `DesignProviders`, so it
  draws rows rather than its loading line.
*/
import { AppFrame, DesignProviders, FolderTree, Grid } from 'lapidary-web'

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

const LIBRARY = '01931b6e-0000-7000-8000-000000000001'

/** `raster.rs`'s own golden render of the fixture bracket — the picture the grid really shows for it. */
const BRACKET_RENDER =
  'data:image/webp;base64,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'

/** One card as `GET /api/libraries/{id}/parts` sends it (the ts-rs `PartCard`). */
function part(
  n: number,
  name: string,
  folder: string,
  triangles: number,
  bytes: number,
  partNumber: string | null,
  thumbnail: string | null = null,
) {
  const id = `01a07c41-5d22-7b03-9014-7e2f6dab0${String(100 + n)}`
  return {
    id,
    library: LIBRARY,
    revision: `01a07c41-5d22-7b03-9014-7e2f6dab1${String(100 + n)}`,
    name,
    partNumber,
    sourcePath: `${folder}/${name}.stl`,
    thumbnail,
    triangleCount: triangles,
    approximate: true,
    sourceHash: null,
    tessellationL0: null,
    sourceBytes: bytes,
    storedBytes: bytes,
    compressed: false,
    directory: `libraries/default/${folder}/${name}`,
    storagePath: `libraries/default/${folder}/${name}/${name}.stl`,
    createdAt: '2026-09-14T09:12:00Z',
    updatedAt: '2026-09-14T09:12:00Z',
    removedAt: null,
  }
}

// Real fixtures from the repo's `fixtures/`, with their real triangle counts and sizes. Only the
// bracket has its render here; the others show the empty well, which is what a part whose preview
// has not been generated yet looks like.
const BRACKET = part(1, 'bracket-lp-1042-03', 'Brackets', 20, 1_084, 'LP-1042-03', BRACKET_RENDER)
const PARTS = [
  BRACKET,
  part(2, 'spacer-lp-2001-00', 'Spacers', 256, 48_714, 'LP-2001-00'),
  part(3, 'idler-bracket-lp-2210-01', 'Brackets', 96, 4_310, 'LP-2210-01'),
  part(4, 'planetary-carrier-lp-3480-02', 'Gearing', 1_842, 92_412, 'LP-3480-02'),
  part(5, 'flange-dn40-lp-3310-02', 'Flanges', 3_216, 160_884, 'LP-3310-02'),
  part(6, 'vee-block-lp-3072-02', 'Fixtures', 148, 7_484, 'LP-3072-02'),
]

const noop = () => {}

const HEADLINE = 'text-2xl leading-tight font-semibold tracking-tight text-[var(--color-bright)]'
const LEAD = 'mt-2 max-w-[70ch] text-sm text-[var(--color-dim)]'
const SECTION = 'mt-10 border-t border-[var(--color-border)] pt-4'
const SECTION_TITLE = 'text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase'
const STANDING =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px'

const CATEGORIES = [
  ['01a07c41-5d22-7b03-9014-7e2f6dab2001', 'Brackets', 14],
  ['01a07c41-5d22-7b03-9014-7e2f6dab2002', 'Fixtures', 9],
  ['01a07c41-5d22-7b03-9014-7e2f6dab2003', 'Flanges', 18],
  ['01a07c41-5d22-7b03-9014-7e2f6dab2004', 'Gearing', 23],
  ['01a07c41-5d22-7b03-9014-7e2f6dab2005', 'Spacers', 31],
].map(([id, name, partCount]) => ({ id, parentId: null, name, slug: name, partCount }))

/** The parts wall: places, search, Upload, the category rail and the grid. */
export function Parts() {
  return (
    <DesignProviders seed={[[['folders', LIBRARY], CATEGORIES]]}>
      <Ground padded={false}>
      <AppFrame
        current="parts"
        library={LIBRARY as never}
        actions={
          <button type="button" className={STANDING} style={{ padding: '6px 12px', fontSize: 12 }}>
            Upload
          </button>
        }
        rail={<FolderTree library={LIBRARY as never} selected={null} onSelect={noop} />}
      >
        <p className="text-[15px] leading-none font-semibold text-[var(--color-bright)]">
          All parts <span className="tabular ml-2 text-xs font-normal text-[var(--color-muted)]">95</span>
        </p>
        <div style={{ marginTop: 16 }}>
          <Grid
            parts={PARTS}
            density="comfortable"
            layout="detail"
            hostRoot={null}
            selecting={false}
            selected={new Set() as never}
            onRender={noop}
            onToggle={noop}
            onSelectAll={noop}
            onOpen={noop}
            onHover={noop}
          />
        </div>
      </AppFrame>
      </Ground>
    </DesignProviders>
  )
}

/** A page past the grid: the sharing page's opening, in `Page.ts`'s anatomy. */
export function Sharing() {
  return (
    <DesignProviders>
      <Ground padded={false}>
      <AppFrame current="sharing" search={null}>
        <section className="max-w-3xl">
          <h2 className={HEADLINE}>Shared libraries</h2>
          <p className={LEAD}>
            Share with people you know. Each of you pastes the other’s device id and address here,
            and your machines talk to each other directly — nothing passes through anybody else’s
            server.
          </p>
          <div className={SECTION}>
            <h3 className={SECTION_TITLE}>This installation</h3>
            <p className="mt-2 text-xs text-[var(--color-muted)]">Device id</p>
            <code className="mt-1 inline-block rounded-[var(--radius-ctl)] bg-[var(--color-raised)] px-2 py-1 font-mono text-sm select-all">
              4JVST-CF8AB-7M2JD-9BCPE-0V5KQ-H8R3N-6WXT1-5SGVA-2FJ4K-3HQ6M-K0
            </code>
          </div>
          <div className={SECTION}>
            <h3 className={SECTION_TITLE}>People you share with</h3>
            <p className="mt-3 text-sm text-[var(--color-muted)]">
              Nobody yet. Pair with somebody by pasting their device id and address.
            </p>
            <button type="button" className={STANDING} style={{ marginTop: 16 }}>
              Pair
            </button>
          </div>
        </section>
      </AppFrame>
      </Ground>
    </DesignProviders>
  )
}
