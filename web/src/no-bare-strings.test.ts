import { parse } from '@babel/parser'
import { expect, test } from 'vitest'
import { strings } from './lib/strings'

/**
 * CLAUDE.md: "No bare user-facing strings in components. English only, but every string
 * goes through `src/lib/strings.ts`."
 *
 * This is a source-level check because it has to be. React Testing Library compares
 * rendered text, so a component hardcoding a byte-identical copy of today's copy renders
 * identically and passes every render test in the suite — provenance is invisible to the
 * DOM. Only reading the source can tell `{strings.parts.approximate}` from
 * `{'Approximate'}`, and the rule this guards is the one that difference breaks: Turkish
 * is the planned second locale, and a string the translator never sees is a string that
 * never gets translated.
 *
 * It reads the AST rather than the text. The regex version this replaces had it backwards
 * in both directions: it missed `<i />Approximate` and `{cond ? 'Yes' : 'No'}` — ordinary
 * React — while failing legitimate code like `` className={`… ${x ? 'a' : 'b'}`} ``,
 * `clsx('flex', busy && 'opacity-50')`, `throw new Error('…')`, and any `if` block
 * following a `Record<string, string>`, which it could not tell from a JSX tag. A gate
 * that fires on correct code gets deleted, and one with seven documented holes is not a
 * gate. `@babel/parser` is already in the tree — `@vitejs/plugin-react` requires it to
 * build the app — so declaring it costs no download; it is pinned in package.json like
 * everything else. The walk is hand-rolled over the parse tree, so `@babel/traverse` is
 * not needed and is not declared.
 *
 * Four positions are user-facing, and everything else is ignored BY CONSTRUCTION rather
 * than by an allowlist — `className`, `data-*`, `style`, `queryKey`, cache keys, thrown
 * error messages and type literals are simply not any of these:
 *
 *   1. JSX text, in any position (`<i />Approximate` included).
 *   2. Any string or template literal inside a JSX expression container that is a CHILD
 *      of an element — however deeply nested, so ternaries, `&&`, call arguments and
 *      arrays are all covered, since each is a way a label reaches the screen.
 *   3. The value of a user-visible attribute: alt, title, aria-label, aria-description,
 *      placeholder, label.
 *   4. Any JSX attribute value at all whose content is a copy of something strings.ts
 *      already says — so `<Badge text={'Approximate'} />` is caught by identity even
 *      though `text` is not a user-visible attribute name.
 *
 * Positions 1-3 also run the cross-check, which reports a copy of a known string as such:
 * that message is the useful one, because copying an existing string is exactly the
 * failure no render test can see.
 *
 * KNOWN BLIND SPOT, stated rather than papered over: a string that is NOT already in
 * strings.ts and reaches the screen through neither JSX text, a child expression, nor a
 * user-visible attribute — `<Badge text={'Exact'} />`, or a `const` rendered later
 * through a variable — is caught by nothing here. Closing it means either an allowlist of
 * technical attribute names (which is what "by construction" exists to avoid) or type
 * information this check does not have.
 *
 * A technical literal written inside a child expression — `{value.toLocaleString('en-US')}`
 * — IS reported. That is intended, not a false positive: formatting user-visible values
 * belongs in strings.ts, and inlining it is how `strings.parts.triangles`' singular branch
 * gets silently dropped.
 */

/**
 * Sources come through Vite's raw glob rather than `node:fs`, which would need
 * `@types/node` — a dependency this project does not have. The pattern is recursive and
 * rooted at `src/`, so a nested route (`routes/parts/detail.tsx`, which is how TanStack
 * file-based routing spells a child route) or a future `components/` directory is scanned
 * the moment it exists. An earlier non-recursive `./*.tsx` rooted at `src/routes/` left
 * both unscanned while every test stayed green.
 *
 * Only `.tsx` is scanned, and that is exhaustive rather than a shortcut: the four
 * positions above are JSX positions, and a `.ts` file cannot contain JSX.
 */
const sources = import.meta.glob('./**/*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<
  string,
  string
>

/** Attributes a user reads or hears. A literal in any of them bypasses strings.ts. */
const USER_VISIBLE_ATTRIBUTES = new Set([
  'alt',
  'title',
  'aria-label',
  'aria-description',
  'placeholder',
  'label',
])

/** Two or more consecutive letters: prose, as opposed to punctuation or whitespace. */
const PROSE = /[A-Za-z]{2,}/

/** Stands in for an interpolated value while a string builder is sampled. */
const SENTINEL = ' {} '

/**
 * Arguments to sample string builders with. More than one type on purpose: a builder
 * takes what it takes, and `strings.parts.triangles` reads `.toLocaleString` while a
 * future `boundingBox: (mm) => mm.toFixed(1)` would reject a string outright. Each call
 * is guarded, because one new entry in strings.ts must never take the gate offline — an
 * unguarded sampler threw at module load and silently dropped five tests from the run.
 */
const SAMPLES: readonly unknown[] = [SENTINEL, 1, 2]

type Node = { type: string; [key: string]: unknown }

function isNode(value: unknown): value is Node {
  return typeof value === 'object' && value !== null && typeof (value as { type?: unknown }).type === 'string'
}

function walk(node: Node, visit: (node: Node, ancestors: readonly Node[]) => void, ancestors: Node[] = []): void {
  visit(node, ancestors)
  const inner = [...ancestors, node]
  for (const [key, value] of Object.entries(node)) {
    if (key === 'loc' || key.endsWith('Comments')) continue
    if (Array.isArray(value)) {
      for (const item of value) if (isNode(item)) walk(item, visit, inner)
    } else if (isNode(value)) {
      walk(value, visit, inner)
    }
  }
}

function addFragment(segment: string, fragments: Set<string>): void {
  const trimmed = segment.trim()
  // Short segments ("mm", ":") collide with ordinary code; only keep ones long enough
  // that finding one in a component means somebody copied it.
  if (trimmed.length >= 5) fragments.add(trimmed)
}

function commonPrefix(a: string, b: string): string {
  let i = 0
  while (i < a.length && i < b.length && a[i] === b[i]) i += 1
  return a.slice(0, i)
}

function commonSuffix(a: string, b: string): string {
  let i = 0
  while (i < a.length && i < b.length && a[a.length - 1 - i] === b[b.length - 1 - i]) i += 1
  return a.slice(a.length - i)
}

/**
 * Every string `strings.ts` can produce. Plain entries go in whole. Function entries are
 * sampled: a production containing the sentinel yields its literal segments directly, and
 * two productions from different arguments yield theirs as their common prefix and suffix
 * — which is how a number-only builder still contributes its words.
 */
export function flattenStrings(source: unknown): { exact: Set<string>; fragments: Set<string> } {
  const exact = new Set<string>()
  const fragments = new Set<string>()

  const visit = (value: unknown): void => {
    if (typeof value === 'string') {
      exact.add(value)
      return
    }
    if (typeof value === 'function') {
      const produced: string[] = []
      for (const sample of SAMPLES) {
        try {
          const result: unknown = (value as (arg: unknown) => unknown)(sample)
          if (typeof result === 'string') produced.push(result)
        } catch {
          // This builder does not take this kind of argument. Try the next sample.
        }
      }
      for (const result of produced) {
        if (result.includes(SENTINEL)) {
          for (const segment of result.split(SENTINEL)) addFragment(segment, fragments)
        } else {
          exact.add(result)
        }
      }
      for (const [index, first] of produced.entries()) {
        for (const second of produced.slice(index + 1)) {
          addFragment(commonPrefix(first, second), fragments)
          addFragment(commonSuffix(first, second), fragments)
        }
      }
      return
    }
    if (value !== null && typeof value === 'object') {
      for (const nested of Object.values(value)) visit(nested)
    }
  }

  visit(source)
  return { exact, fragments }
}

const KNOWN = flattenStrings(strings)

/** Whether this text is something strings.ts already says, whole or in part. */
function copyOfKnownString(value: string): string | undefined {
  const trimmed = value.trim()
  if (trimmed.length === 0) return undefined
  if (KNOWN.exact.has(trimmed)) return trimmed
  return [...KNOWN.fragments].find((fragment) => value.includes(fragment))
}

/** The text a literal node contributes, or undefined for anything else. */
function literalText(node: Node): string | undefined {
  if (node.type === 'StringLiteral') return typeof node.value === 'string' ? node.value : undefined
  if (node.type !== 'TemplateLiteral') return undefined
  const quasis = Array.isArray(node.quasis) ? node.quasis : []
  return quasis
    .map((quasi) => (isNode(quasi) && isNode(quasi.value) ? '' : ((quasi as { value?: { raw?: string } }).value?.raw ?? '')))
    .join(' ')
}

/** The innermost ancestor of this type, or -1. `findLastIndex` is ES2023; lib is ES2022. */
function innermost(ancestors: readonly Node[], type: string): number {
  for (let index = ancestors.length - 1; index >= 0; index -= 1) {
    if (ancestors[index]?.type === type) return index
  }
  return -1
}

function report(found: string[], text: string, position: string): void {
  const copied = copyOfKnownString(text)
  if (copied !== undefined) {
    found.push(`copy of a strings.ts entry: ${copied}`)
    return
  }
  if (PROSE.test(text)) found.push(`${position}: ${text.trim()}`)
}

export function violations(source: string): string[] {
  const ast = parse(source, { sourceType: 'module', plugins: ['typescript', 'jsx'] })
  const found: string[] = []

  walk(ast as unknown as Node, (node, ancestors) => {
    // 1. JSX text, in any child position.
    if (node.type === 'JSXText' && typeof node.value === 'string') {
      report(found, node.value, 'bare JSX text')
      return
    }

    const text = literalText(node)
    if (text === undefined) return

    const attributeIndex = innermost(ancestors, 'JSXAttribute')
    const attribute = attributeIndex === -1 ? undefined : ancestors[attributeIndex]
    if (attribute !== undefined) {
      const name = isNode(attribute.name) ? String(attribute.name.name ?? '') : ''
      // 3. A literal in an attribute the user reads or hears.
      if (USER_VISIBLE_ATTRIBUTES.has(name)) {
        report(found, text, 'user-visible attribute literal')
        return
      }
      // 4. Any attribute at all, when the literal is a copy of a known string.
      const copied = copyOfKnownString(text)
      if (copied !== undefined) found.push(`copy of a strings.ts entry: ${copied}`)
      return
    }

    // 2. A literal anywhere inside an expression container that is a child of an element:
    // `{cond ? 'Yes' : 'No'}`, `{cond && 'Exact'}`, `{fmt('Exact')}`, `{['Exact'].map(f)}`.
    const containerIndex = innermost(ancestors, 'JSXExpressionContainer')
    if (containerIndex <= 0) return
    const parent = ancestors[containerIndex - 1]
    if (parent === undefined) return
    if (parent.type === 'JSXElement' || parent.type === 'JSXFragment') {
      report(found, text, 'string literal rendered as a JSX child')
    }
  })

  return found
}

const componentFiles = Object.keys(sources).filter((path) => !path.endsWith('.test.tsx'))

test('the scan reaches every directory that can render', () => {
  // Each of these would disappear under a different wrong glob: main.tsx if the pattern
  // were rooted at src/routes, the two route files if it were non-recursive. A count
  // floor alone would not have caught either, since one file passing is still a pass.
  expect(componentFiles).toContain('./main.tsx')
  expect(componentFiles).toContain('./routes/index.tsx')
  expect(componentFiles).toContain('./routes/__root.tsx')
  expect(componentFiles.length).toBeGreaterThanOrEqual(3)
  expect(sources['./routes/index.tsx']).toContain('function Card')
})

test('strings.ts was flattened into something to compare against', () => {
  // The cross-check is only as good as this set; empty would make it inert.
  expect(KNOWN.exact.has('Approximate')).toBe(true)
  expect(KNOWN.exact.has('Nothing scanned yet')).toBe(true)
  expect(KNOWN.exact.has('1 triangle')).toBe(true)
  expect(KNOWN.fragments.has('Rendered preview of')).toBe(true)
  expect(KNOWN.fragments.has('triangles')).toBe(true)
})

// A string builder that rejects the sentinel used to throw at module load, taking the
// whole file — and five tests — out of the run with nothing reported as failing. A gate
// that a plausible next commit can silently switch off is not a gate.
test('a string builder that rejects one sample type cannot take the gate offline', () => {
  const hostile = {
    boundingBox: (mm: number) => `${mm.toFixed(1)} mm across`,
    scanned: (at: Date) => `Scanned at ${at.toISOString()}`,
    plain: 'Nothing scanned yet',
  }
  const flattened = flattenStrings(hostile)
  expect(flattened.exact.has('Nothing scanned yet')).toBe(true)
  // Sampled with 1 and 2, whose productions differ only where the number goes.
  expect([...flattened.fragments].some((fragment) => fragment.includes('mm across'))).toBe(true)
})

for (const path of componentFiles) {
  test(`${path} routes every user-facing string through strings.ts`, () => {
    expect(violations(sources[path] ?? '')).toEqual([])
  })
}

// Each position is pinned by the message it produces, not by a violation count, so
// disabling any one of them fails a specific case here instead of being masked by
// another firing on the same input.
test('the scan catches copy in every position that renders', () => {
  const fires = (source: string, prefix: string) =>
    violations(source).some((entry) => entry.startsWith(prefix))

  // The cross-check: byte-identical copies of known strings, single-word ones included.
  expect(fires("const a = <span>{'Approximate'}</span>", 'copy of a strings.ts entry')).toBe(true)
  expect(fires('const a = <p>Nothing scanned yet</p>', 'copy of a strings.ts entry')).toBe(true)
  expect(fires('const a = <img alt={`Rendered preview of ${n}`} />', 'copy of a strings.ts entry')).toBe(true)
  expect(fires("const a = <Badge text={'Approximate'} />", 'copy of a strings.ts entry')).toBe(true)

  // JSX text in a position that is not the first child — the shape that let a copy of
  // the mandated label through the regex version entirely.
  expect(fires('const a = <p><i />Approximate</p>', 'copy of a strings.ts entry')).toBe(true)
  expect(fires('const a = <p><i />Scanning the mounted directory</p>', 'bare JSX text')).toBe(true)
  expect(fires('const a = <p>Scanned {n} files</p>', 'bare JSX text')).toBe(true)

  // Inlining a formatter drops the singular branch of strings.parts.triangles, so a
  // one-triangle part would read "1 triangles".
  expect(fires("const a = <span>{n.toLocaleString('en-US')} triangles</span>", 'copy of a strings.ts entry')).toBe(true)

  // Every ordinary way a short label reaches JSX.
  expect(fires("const a = <p>{ready ? 'Pending' : 'Ready'}</p>", 'string literal rendered as a JSX child')).toBe(true)
  expect(fires("const a = <p>{busy && 'Working'}</p>", 'string literal rendered as a JSX child')).toBe(true)
  expect(fires("const a = <p>{fmt('Working')}</p>", 'string literal rendered as a JSX child')).toBe(true)
  expect(fires("const a = <p>{['Working'].map(f)}</p>", 'string literal rendered as a JSX child')).toBe(true)
  expect(fires("const a = <p>{('Working')}</p>", 'string literal rendered as a JSX child')).toBe(true)

  // User-visible attributes.
  expect(fires('const a = <img alt="a rendered part" />', 'user-visible attribute literal')).toBe(true)
  expect(fires('const a = <span title={detail}>{x}</span>', 'user-visible attribute literal')).toBe(false)
})

// Firing on correct code is the worse failure: that is how a gate gets deleted rather
// than fixed. Every shape here is idiomatic React or TypeScript and must stay silent.
test('the scan ignores everything that is not user-facing', () => {
  const silent = [
    'const a = <p>{strings.parts.loading}</p>',
    'const a = <div className={`flex ${x ? "bg-transparent" : "bg-black"}`} />',
    "const a = <div className={clsx('flex items-center', busy && 'opacity-50')} />",
    "const a = <div data-testid='part-card' style={{ marginTop: '2px' }} />",
    "const a = <input type='button' role='img' id='part' />",
    "throw new Error('fetch is unavailable in this runtime')",
    "const label = 'Parts'",
    "const k = useQuery({ queryKey: ['parts', id] })",
    'const i = `part-name-${id}`',
    "const r = createFileRoute('/')",
    'const m: Record<string, string> = {}\nif (x) { doThing() }',
    "type Mode = 'hobby' | 'controlled'",
    '/* Nothing scanned yet is copy */\nconst a = <p>{x}</p>',
  ]
  for (const source of silent) {
    expect({ source, found: violations(source) }).toEqual({ source, found: [] })
  }
})
