import { parse } from '@babel/parser'
import { expect, test } from 'vitest'

/**
 * Every `<ul>` and `<ol>` in the application carries an explicit `role`.
 *
 * Tailwind's preflight — the reset that arrives with `@import "tailwindcss"`, not the
 * `list-none` utility — sets `list-style: none` on `ul, ol, menu` globally. WebKit
 * responds by dropping the list from the accessibility tree entirely: VoiceOver stops
 * announcing "list, 6 items" and stops offering list navigation, so a category tree and
 * a grid of parts both arrive as an undifferentiated run of buttons. `role="list"` puts
 * back what the reset took, and because the reset is global the rule is global too.
 *
 * A source-level check because it has to be. jsdom implements no accessibility tree and
 * WebKit's behaviour is WebKit's; `getByRole('list')` passes in this test environment
 * whether or not the attribute is there, which makes every render test in the suite blind
 * to exactly this regression. Only the source can tell.
 *
 * Written after a survey that found three lists needing the role and shipped that number:
 * the grep had matched the `list-none` *utility*, which three lists happened to spell out,
 * while the reset applied to all nine. A count taken from the wrong signal agreed with
 * itself and was wrong, so this counts from the parse tree instead of from a reviewer.
 */
const sources = import.meta.glob('./**/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

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

/** Elements the reset strips semantics from. `menu` is in the reset but unused here. */
const LISTS = new Set(['ul', 'ol', 'menu'])

function listsWithoutRole(source: string): number[] {
  const tree = parse(source, {
    sourceType: 'module',
    plugins: ['typescript', 'jsx'],
  }) as unknown as Node
  const lines: number[] = []
  walk(tree, (node) => {
    if (node.type !== 'JSXOpeningElement') return
    const name = node.name as Node | undefined
    if (!name || name.type !== 'JSXIdentifier' || !LISTS.has(String(name.name))) return
    // A spread could carry a role, and reading one statically is guesswork. None of the
    // lists here spread, and a future one that does should say `role` beside the spread
    // rather than teach the gate to assume.
    const attributes = (node.attributes ?? []) as Node[]
    const hasRole = attributes.some(
      (a) => a.type === 'JSXAttribute' && (a.name as Node | undefined)?.name === 'role',
    )
    if (!hasRole) lines.push(((node.loc as { start: { line: number } } | undefined)?.start.line ?? 0))
  })
  return lines
}

test('every list says it is one', () => {
  const offenders = Object.entries(sources)
    .filter(([path]) => !path.endsWith('.test.tsx'))
    .flatMap(([path, source]) => listsWithoutRole(source).map((line) => `${path}:${line}`))

  expect(offenders, 'Tailwind\'s reset strips list semantics; add role="list"').toEqual([])
})

/**
 * The gate has to be able to fail. A pattern that matches nothing is indistinguishable
 * from a codebase with no lists in it, and this suite has been bitten once already by a
 * check that agreed with itself.
 */
test('the gate catches a list that forgot', () => {
  expect(listsWithoutRole('const a = <ul className="x"><li>a</li></ul>')).toEqual([1])
  expect(listsWithoutRole('const a = <ul role="list"><li>a</li></ul>')).toEqual([])
})
