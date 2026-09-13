import { parse } from '@babel/parser'
import { expect, test } from 'vitest'

/**
 * Every page names itself in the tab. WCAG 2.2 SC 2.4.2, Level A.
 *
 * Each route renders its own `<title>`, which React 19 hoists into the head and removes on
 * unmount. The three that exist are asserted in their own test files, one route at a time —
 * which is exactly why this file exists: a fourth route would ship with `index.html`'s one
 * static title, and every existing test would stay green because none of them is about it.
 *
 * A source-level check, and from the parse tree rather than the text. A text search for
 * `<title>` matches the comments explaining the titles, which every route has, so it passes
 * on a route whose only `<title>` is inside a comment.
 *
 * `__root.tsx` is exempt by name: it is TanStack Router's layout, not a page, and it has no
 * topic to title. What a person sees in the tab at the root is `index.html`'s fallback until
 * a page route renders its own.
 */
const routes = import.meta.glob('./routes/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

type Node = { type: string; [key: string]: unknown }

function isNode(value: unknown): value is Node {
  return typeof value === 'object' && value !== null && typeof (value as { type?: unknown }).type === 'string'
}

function some(node: Node, match: (node: Node) => boolean): boolean {
  if (match(node)) return true
  for (const [key, value] of Object.entries(node)) {
    if (key === 'loc' || key.endsWith('Comments')) continue
    if (Array.isArray(value)) {
      if (value.some((item) => isNode(item) && some(item, match))) return true
    } else if (isNode(value) && some(value, match)) {
      return true
    }
  }
  return false
}

/** `export const Route = …` — what makes a file a route rather than a helper beside one. */
function exportsRoute(tree: Node): boolean {
  return some(
    tree,
    (node) =>
      node.type === 'VariableDeclarator' &&
      (node.id as Node | undefined)?.type === 'Identifier' &&
      (node.id as { name?: string }).name === 'Route',
  )
}

function rendersTitle(tree: Node): boolean {
  return some(
    tree,
    (node) =>
      node.type === 'JSXOpeningElement' &&
      (node.name as Node | undefined)?.type === 'JSXIdentifier' &&
      (node.name as { name?: string }).name === 'title',
  )
}

function untitledRoute(source: string): boolean {
  const tree = parse(source, { sourceType: 'module', plugins: ['typescript', 'jsx'] }) as unknown as Node
  return exportsRoute(tree) && !rendersTitle(tree)
}

test('every page route renders its own title', () => {
  const untitled = Object.entries(routes)
    .filter(([path]) => !path.endsWith('.test.tsx') && !path.endsWith('/__root.tsx'))
    .filter(([, source]) => untitledRoute(source))
    .map(([path]) => path)

  expect(untitled, 'render <title>{strings.titles.…}</title> in the route component').toEqual([])
})

/** The glob has to find the routes, or "no untitled routes" is true of an empty list. */
test('the gate is looking at the real routes', () => {
  const paths = Object.keys(routes)
  expect(paths).toContain('./routes/index.tsx')
  expect(paths).toContain('./routes/removed.tsx')
  expect(paths).toContain('./routes/parts.$partId.tsx')
})

/** And it has to be able to fail — including on the comment that fooled a text search. */
test('the gate catches a route that forgot, and is not fooled by a comment', () => {
  const route = (body: string) =>
    `export const Route = createFileRoute('/x')({ component: Page })
     function Page() { return (<section>${body}</section>) }`

  expect(untitledRoute(route('<p>nothing</p>'))).toBe(true)
  expect(untitledRoute(route('{/* a <title> goes here */}<p>nothing</p>'))).toBe(true)
  expect(untitledRoute(route('<title>{strings.titles.library}</title>'))).toBe(false)
  // A helper module beside the routes is not a page.
  expect(untitledRoute('export function helper() { return 1 }')).toBe(false)
})
