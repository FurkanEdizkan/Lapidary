import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { changePassword, fetchProfile, saveProfile } from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD, SECTION, SECTION_TITLE } from '../components/Page'
import { Field, INPUT } from '../components/Doorway'
import { AppFrame } from '../components/AppFrame'

export const Route = createFileRoute('/profile')({
  component: ProfilePage,
})

/** This install's profile (M2): the name it calls its owner. Reached from the user menu. */
export function ProfilePage() {
  const profile = useQuery({ queryKey: ['profile'], queryFn: fetchProfile })
  return (
    <AppFrame current="profile">
      <section className="max-w-3xl">
        <title>{strings.titles.profile}</title>
        <h2 className={HEADLINE}>{strings.profile.title}</h2>
        <p className={LEAD}>{strings.profile.lead}</p>
        {profile.isPending ? (
          <p className="mt-6 text-[var(--color-muted)]">{strings.profile.loading}</p>
        ) : profile.isError ? (
          <p role="alert" className="mt-6 max-w-prose text-[var(--color-muted)]">
            {strings.profile.failed}
          </p>
        ) : (
          <NameForm key={profile.data.displayName ?? ''} initial={profile.data.displayName ?? ''} />
        )}
        <PasswordForm />
      </section>
    </AppFrame>
  )
}

function NameForm({ initial }: { initial: string }) {
  const queryClient = useQueryClient()
  const [name, setName] = useState(initial)
  const save = useMutation({
    mutationFn: () => saveProfile(name),
    onSuccess: (saved) => queryClient.setQueryData(['profile'], saved),
  })
  return (
    <form
      className="mt-6 flex max-w-md flex-col gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (!save.isPending) save.mutate()
      }}
    >
      <label className="flex flex-col gap-1 text-xs text-[var(--color-muted)]">
        {strings.profile.nameLabel}
        <input
          type="text"
          maxLength={80}
          value={name}
          onChange={(event) => setName(event.target.value)}
          className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm text-[var(--color-text)]"
        />
      </label>
      <div className="flex items-center gap-3">
        <button
          type="submit"
          disabled={save.isPending}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {strings.profile.save}
        </button>
        {save.isSuccess ? (
          <span role="status" className="text-sm text-[var(--color-muted)]">
            {strings.profile.saved}
          </span>
        ) : null}
        {save.isError ? (
          <span role="alert" className="text-sm text-[var(--color-muted)]">
            {strings.profile.saveFailed}
          </span>
        ) : null}
      </div>
    </form>
  )
}

/** Your own password (A4), given the one you have now. Your other sessions are signed out. */
function PasswordForm() {
  const [current, setCurrent] = useState('')
  const [password, setPassword] = useState('')
  const change = useMutation({
    mutationFn: () => changePassword({ current, password }),
    onSuccess: () => {
      setCurrent('')
      setPassword('')
    },
  })
  return (
    <section className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.profile.passwordHeading}</h3>
      <form
        className="mt-4 flex max-w-md flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault()
          if (!change.isPending) change.mutate()
        }}
      >
        <Field label={strings.profile.currentPassword}>
          <input type="password" autoComplete="current-password" value={current} onChange={(event) => setCurrent(event.target.value)} className={INPUT} />
        </Field>
        <Field label={strings.profile.newPassword} hint={strings.setup.passwordHint}>
          <input type="password" autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} className={INPUT} />
        </Field>
        <div className="flex items-center gap-3">
          <button
            type="submit"
            disabled={change.isPending || current === '' || password === ''}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.profile.changePassword}
          </button>
          {change.isSuccess ? (
            <span role="status" className="text-sm text-[var(--color-muted)]">
              {strings.profile.passwordChanged}
            </span>
          ) : null}
          {change.isError ? (
            <span role="alert" className="text-sm text-[var(--color-text)]">
              {change.error.message}
            </span>
          ) : null}
        </div>
      </form>
    </section>
  )
}
