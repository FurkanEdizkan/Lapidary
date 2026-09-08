import { render, screen } from '@testing-library/react'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { expect, test, vi } from 'vitest'
import { Crash } from './Crash'
import { strings } from '../lib/strings'

/**
 * Driven through a real router rather than by rendering `<Crash />` directly, because the
 * thing worth checking is not the markup — it is that this component is a valid
 * `defaultErrorComponent`, which is a claim about a prop signature no direct render makes.
 * A boundary that throws on the error it was given is worse than no boundary.
 */
test('a route that throws leaves a page rather than a black screen', async () => {
  // React prints the caught error and the boundary trace. Both are expected here and both
  // are noise; silencing them keeps a passing run readable.
  const error = vi.spyOn(console, 'error').mockImplementation(() => {})

  const rootRoute = createRootRoute({
    component: () => {
      throw new Error('a bug in a render, which is the only thing that reaches here')
    },
  })
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
    defaultErrorComponent: Crash,
  })
  render(<RouterProvider router={router as never} />)

  await screen.findByText(strings.crash.title)
  expect(screen.getByRole('alert')).toBeTruthy()
  // The one action that has ever fixed one of these. A page that only apologises is a
  // black screen with an explanation.
  screen.getByRole('button', { name: strings.crash.reload })

  error.mockRestore()
})
