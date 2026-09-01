import { Outlet, createRootRoute } from '@tanstack/react-router'
import { strings } from '../lib/strings'

export const Route = createRootRoute({
  component: () => (
    <div className="min-h-screen bg-[var(--color-bg)] text-[var(--color-text)]">
      <header className="border-b border-[var(--color-border)] px-6 py-4">
        <h1 className="text-sm font-medium tracking-widest uppercase">{strings.appName}</h1>
      </header>
      <main className="p-6">
        <Outlet />
      </main>
    </div>
  ),
})
