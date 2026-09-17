import { Outlet, createRootRoute } from '@tanstack/react-router'

/**
 * A bare shell. Each page renders its own `AppFrame`, because the grid's header controls are
 * the grid's state (`AppFrame` has why), and the crash page renders one without them.
 */
export const Route = createRootRoute({
  component: () => (
    <div className="min-h-screen bg-[var(--color-bg)] text-[var(--color-text)]">
      <Outlet />
    </div>
  ),
})
