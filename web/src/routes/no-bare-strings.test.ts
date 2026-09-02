import { expect, test } from 'vitest'

/**
 * CLAUDE.md: "No bare user-facing strings in components. English only, but every string
 * goes through `src/lib/strings.ts`."
 *
 * This is a source-level check because it has to be. React Testing Library compares
 * rendered text, so a component hardcoding a byte-identical copy of today's copy renders
 * identically and passes every render test in the suite — provenance is invisible to the
 * DOM. Only reading the source can tell `{strings.parts.approximate}` from
 * `Approximate`, and the rule this guards is exactly the one that difference breaks:
 * Turkish is the planned second locale, and a string the translator never sees is a
 * string that never gets translated.
 *
 * A real TSX parser would be the obvious instrument, but `typescript@7` ships the Go
 * port with no JavaScript AST API, and pulling in a second parser for one check is the
 * wrong trade for a solo-maintained project. So this scans text with three narrow rules,
 * each with a known and deliberately chosen blind spot, and each proven against a
 * mutation rather than assumed.
 */

/**
 * Sources are pulled in through Vite's raw glob rather than `node:fs`, which would need
 * `@types/node` — a dependency this project does not have and does not need for one
 * check. The glob is resolved at transform time, so a renamed or moved route file
 * changes what is scanned without anyone remembering to update a path.
 */
const sources = import.meta.glob('./*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<
  string,
  string
>

/** Attributes a user reads or hears. A literal in any of them bypasses strings.ts. */
const USER_VISIBLE_ATTRIBUTES = /\b(alt|title|aria-label|aria-description|placeholder|label)\s*=\s*\{?\s*['"`]/g

/**
 * JSX text in the first-child position: `<p>Some copy`. Text following a nested element
 * (`<p><b>a</b> tail`) is NOT matched — telling that apart from ordinary code after a
 * closing tag needs a parser. Rule 3 covers the realistic version of that miss, since
 * any such copy long enough to be a sentence contains a space.
 */
const JSX_FIRST_CHILD = /<([A-Za-z][^<>]*)>([^<>{]*)[<{]/g

/** Two or more consecutive letters: prose, as opposed to punctuation or whitespace. */
const PROSE = /[A-Za-z]{2,}/

/** Any quoted or backticked literal, once imports and className values are removed. */
const LITERAL = /'([^'\\\n]*)'|"([^"\\\n]*)"|`([^`\\]*)`/g

function scannable(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, ' ') // block and {/* JSX */} comments
    .replace(/(?<!:)\/\/[^\n]*/g, ' ') // line comments; the guard spares "https://"
    .replace(/^import[\s\S]*?from\s*['"][^'"]*['"]\s*$/gm, ' ') // module specifiers
    .replace(/className\s*=\s*"[^"]*"/g, ' ') // Tailwind class lists are not copy
}

function violations(source: string): string[] {
  const text = scannable(source)
  const found: string[] = []

  for (const match of text.matchAll(USER_VISIBLE_ATTRIBUTES)) {
    found.push(`literal in a user-visible attribute: ${match[0].trim()}`)
  }

  for (const match of text.matchAll(JSX_FIRST_CHILD)) {
    const [, tag = '', child = ''] = match
    if (tag.endsWith('/')) continue // self-closing: there is no child here
    if (PROSE.test(child)) found.push(`bare JSX text: ${child.trim()}`)
  }

  for (const match of text.matchAll(LITERAL)) {
    const value = match[1] ?? match[2] ?? match[3] ?? ''
    if (value.includes(' ') && PROSE.test(value)) found.push(`bare string literal: ${value.trim()}`)
  }

  return found
}

const componentFiles = Object.keys(sources).filter((path) => !path.endsWith('.test.tsx'))

test('there are component files to check', () => {
  // Without this, a glob that matched nothing would make every case below pass by
  // iterating over an empty list.
  expect(componentFiles).toContain('./index.tsx')
  expect(componentFiles).toContain('./__root.tsx')
  expect(sources['./index.tsx']).toContain('function Card')
})

for (const path of componentFiles) {
  test(`${path} routes every user-facing string through strings.ts`, () => {
    expect(violations(sources[path] ?? '')).toEqual([])
  })
}

// The rules have to fire on the shapes they claim to catch, or the case above is just an
// empty array agreeing with itself.
test('the rules catch the shapes they name', () => {
  expect(violations('const a = <p>Loading parts…</p>')).toHaveLength(1)
  expect(violations('const a = <p>Loading {x} parts</p>')).toHaveLength(1)
  expect(violations('const a = <img alt="Rendered preview" />')).toHaveLength(2)
  expect(violations('const a = <img alt={`Rendered preview of ${n}`} />')).toHaveLength(2)
  expect(violations('const a = <span title={detail}>{x}</span>')).toHaveLength(0)
  expect(violations('const t = "No preview yet"')).toHaveLength(1)
  expect(violations('const a = <p>{strings.parts.loading}</p>')).toHaveLength(0)
  expect(violations('const k = ["parts", id]')).toHaveLength(0)
  expect(violations('const i = `part-name-${id}`')).toHaveLength(0)
  expect(violations('/* Loading parts… is copy */')).toHaveLength(0)
})
