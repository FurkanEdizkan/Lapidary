import { expect, test } from 'vitest'

/**
 * The palette keeps the contrast its own comments promise.
 *
 * `styles.css` states measured ratios beside its tokens — "3.01:1 on a card", "all of
 * which clear AA" — and until this file nothing checked them. Two of those promises were
 * found broken by hand during the v2 adoption: the design's fourth text grey measured
 * 3.24:1 where SC 1.4.3 asks 4.5:1, and `--color-edge`, untouched, fell from 3.03:1 to
 * 2.88:1 because the *surfaces* moved up under it. The second is the one that matters. A
 * token can stop meeting its bar without anyone editing it, and a palette change is exactly
 * the edit that arrives without an accessibility review.
 *
 * Read from the source rather than from computed styles, because jsdom applies no
 * stylesheet — `getComputedStyle` answers with defaults here, and a check against defaults
 * passes whatever the palette says.
 */

/**
 * The stylesheet's text, from disk.
 *
 * Not `import.meta.glob('./styles.css', { query: '?raw' })`, which is how the sibling gates
 * read `.tsx`: Vitest blanks every stylesheet, so that import is the empty string, and
 * letting the file through `test.css.include` hands back the processed module instead of the
 * source. Both were tried. The first version of this gate did not pass on the empty read —
 * every token came back undefined and it failed — which is the only reason the problem was
 * visible at all.
 *
 * The specifier is assembled at runtime so TypeScript types the import as `any` rather than
 * looking for `node:fs` declarations this package does not install; the cast names the one
 * call used.
 *
 * The path is relative to Vitest's working directory, `web/`, which is where both
 * `npm --prefix web test` (the xtask gate) and a local `npx vitest` run from. Not resolved
 * from `import.meta.url`: under the jsdom environment that is an `http:` URL, which
 * `readFileSync` refuses. A run from anywhere else fails here with ENOENT naming the path —
 * loudly, rather than by reading nothing.
 */
async function stylesheet(): Promise<string> {
  const fs = (await import(/* @vite-ignore */ ['node', 'fs'].join(':'))) as {
    readFileSync: (path: string, encoding: 'utf8') => string
  }
  return fs.readFileSync('src/styles.css', 'utf8')
}

/** The three grounds text and controls sit on. Every pairing is checked against all three. */
const GROUNDS = ['bg', 'surface', 'raised'] as const

/** Text-bearing tokens, held to SC 1.4.3's 4.5:1 — they carry 9–14px text, so no exemption. */
const TEXT = ['text', 'bright', 'dim', 'muted', 'accent', 'warn', 'good', 'info', 'bad'] as const

/** The boundary of an operable control, held to SC 1.4.11's 3:1. */
const EDGES = ['edge'] as const

/** `--color-<name>: #rrggbb` pairs from the `@theme` block, and nothing outside it. */
function themeColors(source: string): Map<string, string> {
  const start = source.indexOf('@theme')
  const block = start === -1 ? '' : source.slice(start, source.indexOf('\n}', start))
  const colors = new Map<string, string>()
  for (const match of block.matchAll(/--color-([a-z-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) {
    colors.set(match[1]!, match[2]!.toLowerCase())
  }
  return colors
}

/** WCAG 2.x relative luminance of an sRGB hex colour. */
function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = Number.parseInt(hex.slice(i, i + 2), 16) / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!
}

function ratio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi! + 0.05) / (lo! + 0.05)
}

/**
 * Every broken promise, as a sentence naming the token, the ground and the measured ratio —
 * the failure has to say what to fix. A token that has gone missing is a failure too, not a
 * skip: renaming `--color-muted` must not quietly remove it from the check.
 */
function contrastFailures(source: string): string[] {
  const colors = themeColors(source)
  const failures: string[] = []
  // Every missing token named, grounds included. An earlier version reported only the
  // foreground when both were absent, so an unreadable stylesheet — an empty map — listed
  // nine text tokens as undefined and never mentioned that the grounds were gone too, which
  // pointed at the palette when the fault was in reading the file.
  for (const name of [...GROUNDS, ...TEXT, ...EDGES]) {
    if (!colors.has(name)) failures.push(`--color-${name} is not defined in @theme`)
  }
  const check = (names: readonly string[], minimum: number) => {
    for (const name of names) {
      const fg = colors.get(name)
      for (const ground of GROUNDS) {
        const bg = colors.get(ground)
        if (fg === undefined || bg === undefined) continue
        const measured = ratio(fg, bg)
        if (measured < minimum) {
          failures.push(
            `--color-${name} ${fg} on --color-${ground} ${bg} is ${measured.toFixed(2)}:1, needs ${minimum}:1`,
          )
        }
      }
    }
  }
  check(TEXT, 4.5)
  check(EDGES, 3)
  return [...new Set(failures)]
}

test('every text and control token keeps its contrast on every ground', async () => {
  expect(contrastFailures(await stylesheet())).toEqual([])
})

/**
 * The gate has to be able to fail, and on the two cases that motivated it. The first is
 * v2's own caption grey; the second is the old edge under the new surfaces — a token nobody
 * touched, failing because the ground moved.
 */
test('the gate catches the two contrast failures it was written for', () => {
  const theme = (overrides: string) => `@theme {
  --color-bg: #121214;
  --color-surface: #1a1a1d;
  --color-raised: #17171b;
  --color-text: #e6e6e9;
  --color-bright: #f0f0f2;
  --color-dim: #c8c8ce;
  --color-accent: #2cb4f5;
  --color-warn: #e8b06a;
  --color-good: #4f9e94;
  --color-info: #8fd7d0;
  --color-bad: #e88a8a;
  ${overrides}
}`
  expect(contrastFailures(theme('--color-muted: #6a6a72; --color-edge: #65656d;'))).toEqual([
    '--color-muted #6a6a72 on --color-bg #121214 is 3.49:1, needs 4.5:1',
    '--color-muted #6a6a72 on --color-surface #1a1a1d is 3.24:1, needs 4.5:1',
    '--color-muted #6a6a72 on --color-raised #17171b is 3.33:1, needs 4.5:1',
  ])
  // Both lifted grounds, not only the card surface — the adoption was measured against the
  // surface alone and missed that the rail's ground fails too, which is the point of
  // checking every pairing rather than the one somebody thought to try.
  expect(contrastFailures(theme('--color-muted: #8a8a92; --color-edge: #606368;'))).toEqual([
    '--color-edge #606368 on --color-surface #1a1a1d is 2.88:1, needs 3:1',
    '--color-edge #606368 on --color-raised #17171b is 2.96:1, needs 3:1',
  ])
  // A rename is a failure, not a silent pass over a token the check can no longer find.
  expect(contrastFailures(theme('--color-edge: #65656d;'))).toContain(
    '--color-muted is not defined in @theme',
  )
})
