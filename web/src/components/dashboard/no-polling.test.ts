import { parse } from '@babel/parser'
import { expect, test } from 'vitest'

/**
 * The dashboard never polls.
 *
 * `FEATURES.md` §8 calls per-widget polling a self-inflicted DoS, and at twelve widgets it would
 * be exactly that: each one a query, each query on a timer, against a resolve route whose pool is
 * eight connections and whose semaphore is four. `phase-6.md` settles it — **there is no
 * per-widget endpoint, and a web test fails if `refetchInterval` appears under the dashboard's
 * directory.** This is that test, and it covers the two files in `lib/` as well, because
 * `lib/dashboard.ts` is where a timer would be least conspicuous.
 *
 * `setInterval` is refused with it. A `refetchInterval` is the idiomatic way to write this
 * mistake and the one `phase-6.md` names, but a `setInterval` calling `resolve` is the same
 * request pattern with none of the words — and the rule is about the pattern. `setTimeout` is
 * allowed and used: `lib/events.ts` coalesces a burst of notifications with one, which is the
 * opposite of polling, since it makes fewer requests rather than more.
 *
 * A source-level check from the parse tree, for the reason its three siblings are: a render test
 * cannot see a timer that has not fired yet, and a text search matches this very comment.
 */
const sources: Record<string, string> = {
  ...(import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<
    string,
    string
  >),
  ...(import.meta.glob('../../routes/dashboard*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<
    string,
    string
  >),
  ...(import.meta.glob('../../lib/dashboard*.ts', { query: '?raw', import: 'default', eager: true }) as Record<
    string,
    string
  >),
  ...(import.meta.glob('../../lib/events*.ts', { query: '?raw', import: 'default', eager: true }) as Record<
    string,
    string
  >),
}

type Node = { type: string; [key: string]: unknown }

function isNode(value: unknown): value is Node {
  return typeof value === 'object' && value !== null && typeof (value as { type?: unknown }).type === 'string'
}

function walk(node: Node, visit: (node: Node) => void): void {
  visit(node)
  for (const [key, value] of Object.entries(node)) {
    if (key === 'loc' || key.endsWith('Comments')) continue
    if (Array.isArray(value)) {
      for (const item of value) if (isNode(item)) walk(item, visit)
    } else if (isNode(value)) {
      walk(value, visit)
    }
  }
}

/** The name a property or an identifier carries, however it is written. */
function nameOf(node: Node | undefined): string {
  if (node === undefined) return ''
  if (node.type === 'Identifier') return String(node.name ?? '')
  if (node.type === 'StringLiteral') return String(node.value ?? '')
  return ''
}

/** Every polling construct in one source, as `line:what`. */
export function polling(source: string): string[] {
  const tree = parse(source, { sourceType: 'module', plugins: ['typescript', 'jsx'] }) as unknown as Node
  const found: string[] = []
  const line = (node: Node) => (node.loc as { start: { line: number } } | undefined)?.start.line ?? 0
  walk(tree, (node) => {
    // `{ refetchInterval: 5000 }` — the option, under whichever name it is keyed by.
    if (node.type === 'ObjectProperty') {
      const name = nameOf(node.key as Node | undefined)
      if (name.startsWith('refetchInterval')) found.push(`${line(node)}:${name}`)
      return
    }
    // `setInterval(...)`, bare or on an object.
    if (node.type !== 'CallExpression') return
    const callee = node.callee as Node | undefined
    const name =
      callee?.type === 'MemberExpression' ? nameOf(callee.property as Node | undefined) : nameOf(callee)
    if (name === 'setInterval') found.push(`${line(node)}:setInterval`)
  })
  return found
}

const files = Object.keys(sources).filter((path) => !path.includes('.test.'))

/** A pattern that matches nothing passes this gate trivially, which is how it would rot. */
test('the gate is looking at every file the dashboard is made of', () => {
  expect(files).toContain('./Board.tsx')
  expect(files).toContain('./layout.ts')
  expect(files).toContain('./registry.tsx')
  expect(files).toContain('./AddWidget.tsx')
  expect(files).toContain('./widgets.tsx')
  expect(files).toContain('../../routes/dashboard.tsx')
  expect(files).toContain('../../lib/dashboard.ts')
  expect(files).toContain('../../lib/events.ts')
})

test('nothing in the dashboard polls', () => {
  const offenders = files.flatMap((path) => polling(sources[path] ?? '').map((entry) => `${path}:${entry}`))
  expect(offenders, 'the dashboard costs one resolve; events keep it current, not a timer').toEqual([])
})

test('the gate catches every way this would be written', () => {
  expect(polling('const q = useQuery({ queryKey: k, refetchInterval: 5000 })')).toEqual(['1:refetchInterval'])
  expect(polling("const q = useQuery({ 'refetchInterval': 5000 })")).toEqual(['1:refetchInterval'])
  expect(polling('const q = useQuery({ refetchIntervalInBackground: true })')).toEqual([
    '1:refetchIntervalInBackground',
  ])
  expect(polling('setInterval(() => ask(), 1000)')).toEqual(['1:setInterval'])
  expect(polling('window.setInterval(() => ask(), 1000)')).toEqual(['1:setInterval'])
  // And it must stay silent on the one timer this feature does use, and on a comment about it.
  expect(polling('const t = setTimeout(flush, 1000)')).toEqual([])
  expect(polling('/* never a refetchInterval here */\nconst a = 1')).toEqual([])
})
