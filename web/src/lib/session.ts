import type { SessionUser, SetupRequest, SetupStatus, SignIn } from './types'

/**
 * Accounts (A1): who is signed in, the setup wizard's two calls, and signing in and out.
 *
 * **Where a person is sent is decided once, here.** The root route asks {@link gate} before every page;
 * a page that is already open learns a session ended from {@link watchSession}, which looks at every
 * `/api` answer, so no fetch in `api.ts` needs to know about sign-in.
 */

/** The two pages a person reaches without a session. */
export const SETUP_PATH = '/setup'
export const LOGIN_PATH = '/login'

/**
 * A refusal the server sends with a sentence for a person, and its `code` when it sent one, so a page can put the
 * sentence under the field it is about.
 */
export class Refused extends Error {
  constructor(
    message: string,
    readonly code: string | null = null,
  ) {
    super(message)
  }
}

async function refusal(response: Response, fallback: string): Promise<Refused> {
  const body = (await response.json().catch(() => null)) as { message?: string; code?: string } | null
  return new Refused(body?.message ?? fallback, body?.code ?? null)
}

/** `GET /api/session`: the person, or why there is none. */
export async function fetchSession(): Promise<SessionUser | 'setup' | 'signedOut'> {
  const response = await fetch('/api/session')
  if (response.status === 409) return 'setup'
  if (response.status === 401) return 'signedOut'
  if (!response.ok) throw new Error(`session returned ${response.status}`)
  return (await response.json()) as SessionUser
}

/** `GET /api/setup`. */
export async function fetchSetupStatus(): Promise<SetupStatus> {
  const response = await fetch('/api/setup')
  if (!response.ok) throw new Error(`setup status returned ${response.status}`)
  return (await response.json()) as SetupStatus
}

/** `POST /api/setup`: the first account, signed in. A refusal carries the server's sentence. */
export async function runSetup(request: SetupRequest): Promise<SessionUser> {
  const response = await fetch('/api/setup', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(request),
  })
  if (!response.ok) throw await refusal(response, `setup returned ${response.status}`)
  return (await response.json()) as SessionUser
}

/** `POST /api/session`. */
export async function signIn(request: SignIn): Promise<SessionUser> {
  const response = await fetch('/api/session', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(request),
  })
  if (!response.ok) throw await refusal(response, `sign-in returned ${response.status}`)
  return (await response.json()) as SessionUser
}

/** `DELETE /api/session`. */
export async function signOut(): Promise<void> {
  await fetch('/api/session', { method: 'DELETE' })
}

/**
 * Where a page at `path` must send a person instead, or `null` to show it. The answer for a signed-in
 * person is remembered, so moving between pages costs no request; a session that ends is caught by
 * {@link watchSession}.
 */
let signedIn: SessionUser | null = null
export async function gate(path: string): Promise<string | null> {
  if (path === SETUP_PATH || path === LOGIN_PATH) return null
  if (signedIn !== null) return null
  const session = await fetchSession()
  if (session === 'setup') return SETUP_PATH
  if (session === 'signedOut') return LOGIN_PATH
  signedIn = session
  return null
}

/** Who is signed in, once the gate has asked; `null` before that, and in a page's own tests. */
export function currentUser(): SessionUser | null {
  return signedIn
}

/** Forget the remembered answer: after signing out, the next page asks again. */
export function forgetSession() {
  signedIn = null
}

/**
 * Send a person to sign in (or to setup) the moment any `/api` call says their session is gone,
 * rather than leaving each page to show its own "could not load". Installed once, by `main.tsx`.
 */
export function watchSession(go: (path: string) => void) {
  const original = window.fetch.bind(window)
  window.fetch = async (input, init) => {
    const response = await original(input, init)
    const url = typeof input === 'string' ? input : input instanceof URL ? input.pathname : input.url
    const own = url.startsWith('/api/session') || url.startsWith('/api/setup')
    if (!own && (response.status === 401 || response.status === 409) && url.includes('/api/')) {
      const code = ((await response.clone().json().catch(() => null)) as { code?: string } | null)?.code
      if (code === 'signed_out' || code === 'setup_required') {
        forgetSession()
        go(code === 'setup_required' ? SETUP_PATH : LOGIN_PATH)
      }
    }
    return response
  }
}
