import { Outlet, createRootRoute } from '@tanstack/react-router'
import { strings } from '../lib/strings'

export const Route = createRootRoute({
  component: () => (
    <div className="min-h-screen bg-[var(--color-bg)] text-[var(--color-text)]">
      {/*
        The one Display line per viewport, and the only place in the application that raises
        its voice. Weight 900 at −0.02em with a real face behind it: uppercase and letterspaced
        was the old world's way of making a small string look deliberate, and it made the
        wordmark read as a label rather than as a name.
      */}
      <header className="border-b border-[var(--color-border)] px-6 py-4">
        <h1 className="text-[length:clamp(1.75rem,3vw,2rem)] leading-[1.05] font-black tracking-[-0.02em]">
          {strings.appName}
        </h1>
      </header>
      <main className="p-6">
        <Outlet />
      </main>
    </div>
  ),
})
