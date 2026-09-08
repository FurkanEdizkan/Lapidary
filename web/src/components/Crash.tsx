import { strings } from '../lib/strings'

/**
 * What is left when a route throws.
 *
 * Without a boundary a render error unmounts the whole tree and leaves a black page —
 * indistinguishable, to the person looking at it, from the server being down or their
 * library being gone. This is `defaultErrorComponent` on the router: it replaces the
 * failed route and keeps the header, so the application is visibly still there.
 *
 * It does not print the error. "Cannot read properties of undefined" is a sentence about
 * our code, and CLAUDE.md asks that an error say what broke and what to do — the useful
 * half here is the reassurance that nothing on disk was touched, because a person whose
 * parts page just vanished has no way of knowing that. The error itself is in the console,
 * where the person who can act on it is looking.
 */
export function Crash() {
  return (
    <section role="alert" className="mx-auto mt-12 max-w-prose">
      <h2 className="text-xl font-medium">{strings.crash.title}</h2>
      <p className="mt-3 text-sm text-[var(--color-muted)]">{strings.crash.body}</p>
      <button
        type="button"
        onClick={() => window.location.reload()}
        className="ease-mechanical mt-4 rounded border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.crash.reload}
      </button>
    </section>
  )
}
