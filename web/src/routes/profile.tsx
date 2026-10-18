import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { changePassword, fetchProfile, saveProfile } from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD, QUIET, SECTION, SECTION_TITLE, STANDING } from '../components/Page'
import { Field, INPUT } from '../components/Doorway'
import { AppFrame } from '../components/AppFrame'
import { currentUser } from '../lib/session'

export const Route = createFileRoute('/profile')({
  component: ProfilePage,
})

/** `QUIET` at a form's size, so a quiet verb stands as tall as `STANDING` would in its place. */
const QUIET_FORM = QUIET.replace('min-h-8', 'min-h-9').replace('text-xs', 'text-sm')

/** This install's profile (M2): the name it calls its owner. Reached from the user menu. */
export function ProfilePage() {
  const profile = useQuery({ queryKey: ['profile'], queryFn: fetchProfile })
  return (
    <AppFrame current="profile">
      <section className="max-w-3xl">
        <title>{strings.titles.profile}</title>
        <h2 className={HEADLINE}>{strings.profile.title}</h2>
        <p className={LEAD}>{strings.profile.lead}</p>
        {/* The name is the page's subject, so it sits under the lead with no label over it to repeat its own. */}
        {profile.isPending ? (
          <p className="mt-6 text-sm text-[var(--color-muted)]">{strings.profile.loading}</p>
        ) : profile.isError ? (
          <p role="alert" className="mt-6 max-w-[70ch] text-sm text-[var(--color-muted)]">
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
  // With no name of your own, the field shows what Lapidary calls you instead: your username (A17's audit).
  const username = currentUser()?.username
  const changed = name !== initial
  const save = useMutation({
    mutationFn: () => saveProfile(name),
    onSuccess: (saved) => queryClient.setQueryData(['profile'], saved),
  })
  return (
    <form
      className="mt-6 flex max-w-md flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (!save.isPending) save.mutate()
      }}
    >
      <Field label={strings.profile.nameLabel}>
        <input
          type="text"
          maxLength={80}
          autoComplete="name"
          value={name}
          placeholder={username}
          onChange={(event) => setName(event.target.value)}
          className={INPUT}
        />
      </Field>
      <div className="flex items-center gap-3">
        {/* The page's one standing verb once there is a change to save; quiet, and still pressable, until then. */}
        <button type="submit" disabled={save.isPending} className={changed ? STANDING : QUIET_FORM}>
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
            className={QUIET_FORM}
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
