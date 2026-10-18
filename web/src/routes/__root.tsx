import { Outlet, createRootRoute, redirect } from '@tanstack/react-router'
import { MotionRoot } from '../lib/motion-react'
import { gate } from '../lib/session'

/**
 * A bare shell. Each page renders its own `AppFrame`, because the grid's header controls are
 * the grid's state (`AppFrame` has why), and the crash page renders one without them.
 *
 * Before any page, the session gate (A1): nobody with an account yet goes to setup, a person who
 * is not signed in goes to sign in.
 *
 * `MotionRoot` (A18) gives every page Motion's tokens and its answer to reduced motion.
 */
export const Route = createRootRoute({
  beforeLoad: async ({ location }) => {
    const to = await gate(location.pathname)
    if (to !== null) throw redirect({ to })
  },
  component: () => (
    <div className="min-h-screen bg-[var(--color-bg)] text-[var(--color-text)]">
      <MotionRoot>
        <Outlet />
      </MotionRoot>
    </div>
  ),
})
