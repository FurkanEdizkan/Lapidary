import { createFileRoute } from '@tanstack/react-router'
import { useMutation } from '@tanstack/react-query'
import { Suspense, lazy, useState } from 'react'
import { Doorway, Field, INPUT } from '../components/Doorway'
import { HEADLINE, LEAD, STANDING } from '../components/Page'
import { Refused, signIn } from '../lib/session'
import { strings } from '../lib/strings'

/** Lazy, so the wall loads with this page and never with the grid. */
const Wall = lazy(() => import('../components/Wall'))

export const Route = createFileRoute('/login')({
  component: LoginPage,
})

/** Signing in (A1). A wrong password says so in the server's sentence; anything else in ours. */
export function LoginPage() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const submit = useMutation({
    mutationFn: () => signIn({ username, password }),
    // A full load rather than a navigation: every query cached under the last person, if any, goes.
    onSuccess: () => window.location.assign('/'),
  })
  return (
    <Doorway
      backdrop={
        <Suspense fallback={null}>
          <Wall />
        </Suspense>
      }
    >
      <title>{strings.titles.login}</title>
      <h2 className={HEADLINE}>{strings.login.title}</h2>
      <p className={LEAD}>{strings.login.lead}</p>
      <form
        className="mt-8 flex flex-col gap-5"
        onSubmit={(event) => {
          event.preventDefault()
          if (!submit.isPending) submit.mutate()
        }}
      >
        <Field label={strings.login.usernameLabel}>
          <input
            type="text"
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
            required
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            className={INPUT}
          />
        </Field>
        <Field label={strings.login.passwordLabel}>
          <input
            type="password"
            autoComplete="current-password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            className={INPUT}
          />
        </Field>
        {submit.isError ? (
          <p role="alert" className="text-sm text-[var(--color-text)]">
            {submit.error instanceof Refused ? submit.error.message : strings.login.failed}
          </p>
        ) : null}
        <div className="mt-1 flex flex-col items-start gap-4">
          <button
            type="submit"
            disabled={submit.isPending}
            className={`${STANDING} whitespace-nowrap disabled:opacity-50`}
          >
            {submit.isPending ? strings.login.signingIn : strings.login.submit}
          </button>
          <p className="text-xs text-[var(--color-muted)]">{strings.login.forgot}</p>
        </div>
      </form>
    </Doorway>
  )
}
