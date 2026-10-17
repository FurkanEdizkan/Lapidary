import { afterEach, expect, test, vi } from 'vitest'

// The real module: `test-setup.ts` stubs the gate for every page test.
const session = await vi.importActual<typeof import('./session')>('./session')

afterEach(() => {
  vi.unstubAllGlobals()
  session.forgetSession()
})

function answer(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

test('the gate sends nobody to setup, a stranger to sign in, and asks once for a signed-in person', async () => {
  const fetchMock = vi.fn(async () => answer(409, { code: 'setup_required' }))
  vi.stubGlobal('fetch', fetchMock)
  expect(await session.gate('/')).toBe(session.SETUP_PATH)
  // The two doorways never ask.
  expect(await session.gate(session.SETUP_PATH)).toBeNull()
  expect(fetchMock).toHaveBeenCalledTimes(1)

  fetchMock.mockImplementation(async () => answer(401, { code: 'signed_out' }))
  expect(await session.gate('/dashboard')).toBe(session.LOGIN_PATH)

  fetchMock.mockImplementation(async () =>
    answer(200, { id: '01931b6e-0000-7000-8000-00000000a001', username: 'lena', displayName: null, isAdmin: true }),
  )
  expect(await session.gate('/')).toBeNull()
  expect(await session.gate('/tags')).toBeNull()
  expect(fetchMock).toHaveBeenCalledTimes(3)
})

test('an api answer that says the session ended sends the person to sign in', async () => {
  const go = vi.fn()
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) =>
      url === '/api/libraries' ? answer(401, { code: 'signed_out' }) : answer(401, { code: 'wrong_password' }),
    ),
  )
  session.watchSession(go)
  await fetch('/api/libraries')
  expect(go).toHaveBeenCalledWith(session.LOGIN_PATH)
  // A wrong password on the sign-in page is that page's to say, not a redirect.
  go.mockClear()
  await fetch('/api/session', { method: 'POST' })
  expect(go).not.toHaveBeenCalled()
})
