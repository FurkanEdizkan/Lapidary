import { createFileRoute } from '@tanstack/react-router'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { Doorway, Field, INPUT } from '../components/Doorway'
import { HEADLINE, LEAD, STANDING } from '../components/Page'
import { Refused, signIn } from '../lib/session'
import { strings } from '../lib/strings'

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
    <Doorway>
      <title>{strings.titles.login}</title>
      <h2 className={HEADLINE}>{strings.login.title}</h2>
      <p className={LEAD}>{strings.login.lead}</p>
      <form
        className="mt-8 flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault()
          if (!submit.isPending) submit.mutate()
        }}
      >
        <Field label={strings.login.usernameLabel}>
          <input
            type="text"
            autoComplete="username"
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
        <div className="flex items-center justify-between gap-3">
          <button type="submit" disabled={submit.isPending} className={`${STANDING} disabled:opacity-50`}>
            {submit.isPending ? strings.login.signingIn : strings.login.submit}
          </button>
          <span className="text-xs text-[var(--color-dim)]">{strings.login.forgot}</span>
        </div>
      </form>
    </Doorway>
  )
}
