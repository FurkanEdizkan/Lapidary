import { strings } from '../lib/strings'
import { AppFrame } from './AppFrame'
import { HEADLINE, LEAD, STANDING } from './Page'

/**
 * What is left when a route throws.
 *
 * Without a boundary a render error unmounts the whole tree and leaves a black page —
 * indistinguishable, to the person looking at it, from the server being down or their
 * library being gone. This is `defaultErrorComponent` on the router: it replaces the
 * failed route with a frame of its own, so the application is visibly still there. The
 * frame has the mark and nothing that reads the router or a library, because the failure
 * could be in either.
 *
 * It does not print the error. "Cannot read properties of undefined" is a sentence about
 * our code, and CLAUDE.md asks that an error say what broke and what to do — the useful
 * half here is the reassurance that nothing on disk was touched, because a person whose
 * parts page just vanished has no way of knowing that. The error itself is in the console,
 * where the person who can act on it is looking.
 */
export function Crash() {
  return (
    <AppFrame nav={false} search={null}>
      <section role="alert" className="mx-auto mt-12 max-w-prose">
        <h2 className={HEADLINE}>{strings.crash.title}</h2>
        <p className={LEAD}>{strings.crash.body}</p>
        <button
          type="button"
          onClick={() => window.location.reload()}
          className={`mt-6 ${STANDING}`}
        >
          {strings.crash.reload}
        </button>
      </section>
    </AppFrame>
  )
}
