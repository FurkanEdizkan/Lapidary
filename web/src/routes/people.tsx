import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { AppFrame } from '../components/AppFrame'
import { ConfirmDelete } from '../components/ConfirmDelete'
import { Field, INPUT } from '../components/Doorway'
import { HEADLINE, LEAD, SECTION, SECTION_TITLE, STANDING } from '../components/Page'
import { createPerson, deletePerson, fetchPeople, resetPassword } from '../lib/api'
import { currentUser } from '../lib/session'
import { Presence } from '../lib/motion-react'
import { strings } from '../lib/strings'
import type { Person } from '../lib/types'

export const Route = createFileRoute('/people')({
  component: PeoplePage,
})

const QUIET =
  'ease-mechanical text-sm text-[var(--color-muted)] underline-offset-2 duration-[var(--duration-fast)] hover:text-[var(--color-text)] hover:underline'

/**
 * The people on this install (A4). Everybody may see who is here — it is who they can share with —
 * and an admin adds people, gives them a new password, and removes them. A removed person's libraries
 * become the admin's, so nothing they added is lost.
 */
export function PeoplePage() {
  const people = useQuery({ queryKey: ['people'], queryFn: fetchPeople })
  const me = currentUser()
  const admin = me?.isAdmin ?? false
  const [note, setNote] = useState<string | null>(null)
  return (
    <AppFrame current="people">
      <section className="max-w-3xl">
        <title>{strings.titles.people}</title>
        <h2 className={HEADLINE}>{strings.people.title}</h2>
        <p className={LEAD}>{strings.people.lead}</p>
        {note === null ? null : (
          <p role="status" className="mt-4 text-sm text-[var(--color-text)]">
            {note}
          </p>
        )}
        <section className={SECTION} aria-labelledby="people-accounts">
          <h3 id="people-accounts" className={SECTION_TITLE}>
            {strings.people.listHeading}
          </h3>
          {people.isPending ? (
            <p className="mt-4 text-sm text-[var(--color-muted)]">{strings.people.loading}</p>
          ) : people.isError ? (
            <p role="alert" className="mt-4 max-w-[70ch] text-sm text-[var(--color-muted)]">
              {strings.people.failed}
            </p>
          ) : (
            // Rows divided, not boxed: the last one has no rule of its own above the next section's (A17's audit).
            <ul role="list" className="flex flex-col divide-y divide-[var(--color-border)]">
              {people.data.map((person) => (
                <PersonRow key={person.id} person={person} you={person.id === me?.id} admin={admin} onNote={setNote} />
              ))}
            </ul>
          )}
        </section>
        {admin ? (
          <AddPerson onNote={setNote} />
        ) : (
          <p className="mt-6 max-w-[70ch] text-sm text-[var(--color-dim)]">{strings.people.adminsOnly}</p>
        )}
      </section>
    </AppFrame>
  )
}

function PersonRow({
  person,
  you,
  admin,
  onNote,
}: {
  person: Person
  you: boolean
  admin: boolean
  onNote: (note: string) => void
}) {
  const queryClient = useQueryClient()
  const [resetting, setResetting] = useState(false)
  const [password, setPassword] = useState('')
  const [removing, setRemoving] = useState(false)
  const name = person.displayName ?? person.username
  const reset = useMutation({
    mutationFn: () => resetPassword(person.id, password),
    onSuccess: () => {
      setResetting(false)
      setPassword('')
      onNote(strings.people.resetDone(name))
    },
  })
  const remove = useMutation({
    mutationFn: () => deletePerson(person.id),
    onSuccess: () => {
      setRemoving(false)
      void queryClient.invalidateQueries({ queryKey: ['people'] })
      void queryClient.invalidateQueries({ queryKey: ['libraries'] })
    },
  })
  return (
    <li className="flex flex-col gap-2 py-3">
      {/*
        Each value says what it is (A17's audit read "rig rig admin you"): the name in Title, then a labelled username
        and role. The username is drawn only when it is not already the name above it.
      */}
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="text-sm font-semibold text-[var(--color-bright)]">{name}</span>
        {you ? <span className="text-xs text-[var(--color-muted)]">{strings.people.you}</span> : null}
        {admin && !you ? (
          <span className="ml-auto flex gap-3">
            <button type="button" onClick={() => setResetting(!resetting)} className={QUIET}>
              {strings.people.reset}
            </button>
            <button type="button" onClick={() => setRemoving(true)} className={QUIET}>
              {strings.people.remove}
            </button>
          </span>
        ) : null}
      </div>
      <dl className="flex flex-wrap gap-x-6 gap-y-1 text-xs">
        {person.displayName === null ? null : (
          <div className="flex items-baseline gap-2">
            <dt className="text-[var(--color-muted)]">{strings.people.username}</dt>
            <dd className="font-mono text-[var(--color-dim)]">{person.username}</dd>
          </div>
        )}
        <div className="flex items-baseline gap-2">
          <dt className="text-[var(--color-muted)]">{strings.people.role}</dt>
          <dd className="text-[var(--color-dim)]">{person.isAdmin ? strings.people.admin : strings.people.member}</dd>
        </div>
      </dl>
      {resetting ? (
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (!reset.isPending) reset.mutate()
          }}
        >
          <Field label={strings.people.resetPrompt(name)}>
            <input
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              className={INPUT}
            />
          </Field>
          <button type="submit" disabled={reset.isPending} className={STANDING}>
            {strings.people.reset}
          </button>
          {reset.isError ? (
            <p role="alert" className="w-full text-sm text-[var(--color-text)]">
              {reset.error.message}
            </p>
          ) : null}
        </form>
      ) : null}
      <Presence>
        {removing ? (
          <ConfirmDelete
            key="remove"
            title={strings.people.removeTitle(name)}
            body={strings.people.removeBody(name)}
            busy={remove.isPending}
            failure={remove.isError ? remove.error.message : null}
            onConfirm={() => remove.mutate()}
            onCancel={() => setRemoving(false)}
          />
        ) : null}
      </Presence>
    </li>
  )
}

function AddPerson({ onNote }: { onNote: (note: string) => void }) {
  const queryClient = useQueryClient()
  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  const [isAdmin, setIsAdmin] = useState(false)
  const add = useMutation({
    mutationFn: () =>
      createPerson({
        username,
        displayName: displayName.trim() === '' ? null : displayName,
        password,
        isAdmin,
      }),
    onSuccess: (person) => {
      onNote(strings.people.added(person.displayName ?? person.username))
      setUsername('')
      setDisplayName('')
      setPassword('')
      setIsAdmin(false)
      void queryClient.invalidateQueries({ queryKey: ['people'] })
    },
  })
  return (
    <section className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.people.addHeading}</h3>
      <form
        className="mt-4 flex max-w-[28rem] flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault()
          if (!add.isPending) add.mutate()
        }}
      >
        <Field label={strings.people.username}>
          <input type="text" autoComplete="off" spellCheck={false} value={username} onChange={(event) => setUsername(event.target.value)} className={INPUT} />
        </Field>
        <Field label={strings.people.displayName}>
          <input type="text" autoComplete="off" value={displayName} onChange={(event) => setDisplayName(event.target.value)} className={INPUT} />
        </Field>
        <Field label={strings.people.password} hint={strings.people.passwordHint}>
          <input type="password" autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} className={INPUT} />
        </Field>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={isAdmin} onChange={(event) => setIsAdmin(event.target.checked)} className="size-4 accent-[var(--color-accent)]" />
          {strings.people.makeAdmin}
        </label>
        {add.isError ? (
          <p role="alert" className="text-sm text-[var(--color-text)]">
            {add.error.message}
          </p>
        ) : null}
        <div className="mt-1">
          <button type="submit" disabled={add.isPending} className={STANDING}>
            {add.isPending ? strings.people.adding : strings.people.add}
          </button>
        </div>
      </form>
    </section>
  )
}
