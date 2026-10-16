import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { fetchProfile, saveProfile } from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD } from '../components/Page'
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
