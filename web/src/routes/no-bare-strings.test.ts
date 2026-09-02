import { expect, test } from 'vitest'
import { strings } from '../lib/strings'

/**
 * CLAUDE.md: "No bare user-facing strings in components. English only, but every string
 * goes through `src/lib/strings.ts`."
 *
 * This is a source-level check because it has to be. React Testing Library compares
 * rendered text, so a component hardcoding a byte-identical copy of today's copy renders
 * identically and passes every render test in the suite — provenance is invisible to the
 * DOM. Only reading the source can tell `{strings.parts.approximate}` from
 * `{'Approximate'}`, and the rule this guards is exactly the one that difference breaks:
 * Turkish is the planned second locale, and a string the translator never sees is a
 * string that never gets translated.
 *
 * A real TSX parser would be the obvious instrument, but `typescript@7` ships the Go
 * port with no JavaScript AST API, and pulling in a second parser for one check is the
 * wrong trade for a solo-maintained project. So this scans text with five narrow rules
 * in two complementary groups:
 *
 *   - The CROSS-CHECK (rule 4) compares every literal in a component against the actual
 *     values in `strings.ts`. It is exact where a heuristic can only guess, and it is
 *     the rule that matters most, because copying an existing string is the failure mode
 *     no render test can see.
 *   - The SHAPE rules (1, 2, 3, 5) catch copy that never went through `strings.ts` at
 *     all, which the cross-check cannot know about.
 *
 * KNOWN BLIND SPOT, stated rather than papered over: a single-word literal that is NOT
 * in `strings.ts` and sits in neither a JSX child position nor a user-visible attribute
 * — `<Badge text={'Exact'} />`, or a `const` used later — is caught by nothing here.
 * Rule 3 needs a space, rule 4 needs the string to be known, rules 2 and 5 need a child
 * position. An earlier version of this file claimed coverage it did not have, and that
 * claim was worse than the gap: `{'Approximate'}` — the exact label CLAUDE.md's
 * non-negotiable exists to produce — passed the whole suite.
 *
 * Test files are deliberately not scanned. The literal content pins in
 * `index.test.tsx` are there on purpose, for the reason that file documents.
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

/** Stands in for an interpolated value while a string builder is being sampled. */
const SENTINEL = ' {} '

/** Attributes a user reads or hears. A literal in any of them bypasses strings.ts. */
const USER_VISIBLE_ATTRIBUTES = /\b(alt|title|aria-label|aria-description|placeholder|label)\s*=\s*\{?\s*['"`]/g

/**
 * JSX text in the first-child position: `<p>Some copy`. Text following a nested element
 * (`<p><b>a</b> tail`) is NOT matched — telling that apart from ordinary code after a
 * closing tag needs a parser.
 */
const JSX_FIRST_CHILD = /<([A-Za-z][^<>]*)>([^<>{]*)[<{]/g

/**
 * A literal handed straight to JSX as a child: `<span>{'Approximate'}</span>`. Anchored
 * on `}` as well as `>`, so a literal sitting after another expression rather than first
 * — `{count}{'Beta'}` — is caught too; that arrangement slipped through when this only
 * looked at the first child position.
 */
const JSX_CHILD_LITERAL = /[>}]\s*\{\s*['"`]/g

/** Any quoted or backticked literal, once imports and className values are removed. */
const LITERAL = /'([^'\\\n]*)'|"([^"\\\n]*)"|`([^`\\]*)`/g

/** Two or more consecutive letters: prose, as opposed to punctuation or whitespace. */
const PROSE = /[A-Za-z]{2,}/

/**
 * Every string `strings.ts` can produce. Plain values are collected whole; the function
 * entries are sampled with a sentinel and with `1`, so an interpolated string
 * contributes its literal segments ("Rendered preview of", "triangles") — which is what
 * a component copying it would write around its own expression.
 */
function knownStrings(): { exact: Set<string>; fragments: Set<string> } {
  const exact = new Set<string>()
  const fragments = new Set<string>()

  const walk = (value: unknown): void => {
    if (typeof value === 'string') {
      exact.add(value)
      return
    }
    if (typeof value === 'function') {
      for (const sample of [SENTINEL, 1]) {
        const produced: unknown = (value as (arg: unknown) => unknown)(sample)
        if (typeof produced !== 'string') continue
        if (!produced.includes(SENTINEL)) {
          exact.add(produced)
          continue
        }
        for (const segment of produced.split(SENTINEL)) {
          const trimmed = segment.trim()
          // Short segments ("mm", ":") collide with ordinary code; only keep segments
          // long enough that finding one in a component means someone copied it.
          if (trimmed.length >= 5) fragments.add(trimmed)
        }
      }
      return
    }
    if (value !== null && typeof value === 'object') {
      for (const nested of Object.values(value)) walk(nested)
    }
  }

  walk(strings)
  return { exact, fragments }
}

const KNOWN = knownStrings()

function scannable(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, ' ') // block and {/* JSX */} comments
    .replace(/(?<!:)\/\/[^\n]*/g, ' ') // line comments; the guard spares "https://"
    .replace(/^import[\s\S]*?from\s*['"][^'"]*['"]\s*$/gm, ' ') // module specifiers
    .replace(/className\s*=\s*"[^"]*"/g, ' ') // Tailwind class lists are not copy
}

/**
 * Rule 4. Whether this fragment of source is a copy of something `strings.ts` already
 * says — either the whole string, or the literal part of an interpolated one. Applied to
 * both JSX text and quoted literals, since a component can hardcode copy either way.
 */
function copyOfKnownString(value: string): string | undefined {
  const trimmed = value.trim()
  if (trimmed.length === 0) return undefined
  if (KNOWN.exact.has(trimmed)) return trimmed
  return [...KNOWN.fragments].find((fragment) => value.includes(fragment))
}

export function violations(source: string): string[] {
  const text = scannable(source)
  const found: string[] = []

  for (const match of text.matchAll(USER_VISIBLE_ATTRIBUTES)) {
    found.push(`user-visible attribute literal: ${match[0].trim()}`)
  }

  for (const match of text.matchAll(JSX_FIRST_CHILD)) {
    const [, tag = '', child = ''] = match
    if (tag.endsWith('/')) continue // self-closing: there is no child here
    const copied = copyOfKnownString(child)
    if (copied !== undefined) {
      found.push(`copy of a strings.ts entry: ${copied}`)
      continue
    }
    if (PROSE.test(child)) found.push(`bare JSX text: ${child.trim()}`)
  }

  for (const match of text.matchAll(JSX_CHILD_LITERAL)) {
    found.push(`string literal rendered as a JSX child: ${match[0].trim()}`)
  }

  for (const match of text.matchAll(LITERAL)) {
    const value = match[1] ?? match[2] ?? match[3] ?? ''
    const copied = copyOfKnownString(value)
    if (copied !== undefined) {
      found.push(`copy of a strings.ts entry: ${copied}`)
      continue
    }
    if (value.includes(' ') && PROSE.test(value)) found.push(`bare prose literal: ${value.trim()}`)
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

test('strings.ts was flattened into something to compare against', () => {
  // The cross-check is only as good as this set; empty would make rule 4 inert.
  expect(KNOWN.exact.has('Approximate')).toBe(true)
  expect(KNOWN.exact.has('Nothing scanned yet')).toBe(true)
  expect(KNOWN.fragments.has('Rendered preview of')).toBe(true)
  expect(KNOWN.fragments.has('triangles')).toBe(true)
})

for (const path of componentFiles) {
  test(`${path} routes every user-facing string through strings.ts`, () => {
    expect(violations(sources[path] ?? '')).toEqual([])
  })
}

// Each rule is pinned by the message it produces, not by a count, so disabling any one
// of them fails a specific case here instead of being masked by another rule firing.
test('the rules catch the shapes they name', () => {
  const fires = (source: string, prefix: string) =>
    violations(source).some((entry) => entry.startsWith(prefix))

  // Rule 4, the cross-check: byte-identical copies of known strings, single-word ones
  // included. This is the case that motivated the whole file.
  expect(fires("const a = <span>{'Approximate'}</span>", 'copy of a strings.ts entry')).toBe(true)
  expect(fires('const a = <p>Nothing scanned yet</p>', 'copy of a strings.ts entry')).toBe(true)
  expect(fires('const a = <img alt={`Rendered preview of ${n}`} />', 'copy of a strings.ts entry')).toBe(true)
  expect(fires('const t = `${n} triangles`', 'copy of a strings.ts entry')).toBe(true)

  // Rule 1: user-visible attributes.
  expect(fires('const a = <img alt="a rendered part" />', 'user-visible attribute literal')).toBe(true)
  expect(fires('const a = <span title={detail}>{x}</span>', 'user-visible attribute literal')).toBe(false)

  // Rule 2: bare JSX text, including text beside an expression.
  expect(fires('const a = <p>Scanning the mounted directory</p>', 'bare JSX text')).toBe(true)
  expect(fires('const a = <p>Scanned {n} files</p>', 'bare JSX text')).toBe(true)

  // Rule 5: any literal handed to JSX as a child, however short, first or not.
  expect(fires("const a = <span>{'Exact'}</span>", 'string literal rendered as a JSX child')).toBe(true)
  expect(fires("const a = <span>{n}{'Beta'}</span>", 'string literal rendered as a JSX child')).toBe(true)

  // Rule 3: unknown prose anywhere in the file.
  expect(fires("const t = 'Scanning the mounted directory'", 'bare prose literal')).toBe(true)

  // Ordinary code must stay silent, or the check gets switched off out of annoyance.
  expect(violations('const a = <p>{strings.parts.loading}</p>')).toEqual([])
  expect(violations('const k = ["parts", id]')).toEqual([])
  expect(violations('const i = `part-name-${id}`')).toEqual([])
  expect(violations('const r = createFileRoute("/")')).toEqual([])
  expect(violations('/* Nothing scanned yet is copy */')).toEqual([])
})
