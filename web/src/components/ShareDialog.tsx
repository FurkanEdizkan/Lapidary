import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { fetchPeople, fetchShares, saveShares } from '../lib/api'
import { currentUser } from '../lib/session'
import { strings } from '../lib/strings'
import type { PartId, UserId } from '../lib/types'
import { Dialog } from './Dialog'

const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * Who sees a model (A4): everybody on this install, chosen people, or nobody but its owner. A share is
 * to look and download; changing the model stays the owner's.
 *
 * One part opens on its current shares. Several — the grid's selection — open on nobody, and saving
 * gives every one of them exactly what is ticked.
 */
export function ShareDialog({ parts, onClose }: { parts: PartId[]; onClose: () => void }) {
  const queryClient = useQueryClient()
  const one = parts.length === 1 ? parts[0] : undefined
  const people = useQuery({ queryKey: ['people'], queryFn: fetchPeople })
  const current = useQuery({
    queryKey: ['shares', one],
    queryFn: () => fetchShares(one as PartId),
    enabled: one !== undefined,
  })
  const [everyone, setEveryone] = useState<boolean | null>(null)
  const [chosen, setChosen] = useState<UserId[] | null>(null)
  const isEveryone = everyone ?? current.data?.everyone ?? false
  const picked = chosen ?? current.data?.people ?? []
  const me = currentUser()?.id
  const others = (people.data ?? []).filter((person) => person.id !== me)

  const save = useMutation({
    mutationFn: async () => {
      for (const part of parts) await saveShares(part, { everyone: isEveryone, people: picked })
    },
    onSuccess: () => {
      for (const part of parts) void queryClient.invalidateQueries({ queryKey: ['shares', part] })
      onClose()
    },
  })
  const toggle = (id: UserId) =>
    setChosen(picked.includes(id) ? picked.filter((p) => p !== id) : [...picked, id])

  return (
    <Dialog title={strings.sharing.title(parts.length)} onClose={onClose}>
      <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">{strings.sharing.lead}</p>
      <label className="mt-4 flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={isEveryone}
          onChange={(event) => setEveryone(event.target.checked)}
          className="size-4 accent-[var(--color-accent)]"
        />
        {strings.sharing.everyone}
      </label>
      <fieldset className="mt-4">
        <legend className="text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase">
          {strings.sharing.people}
        </legend>
        {people.isPending ? (
          <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
        ) : others.length === 0 ? (
          <p className="mt-2 text-sm text-[var(--color-dim)]">{strings.sharing.nobodyElse}</p>
        ) : (
          <ul role="list" className="mt-2 flex flex-col gap-1">
            {others.map((person) => (
              <li key={person.id}>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={isEveryone || picked.includes(person.id)}
                    disabled={isEveryone}
                    onChange={() => toggle(person.id)}
                    className="size-4 accent-[var(--color-accent)]"
                  />
                  {person.displayName ?? person.username}
                  <span className="text-xs text-[var(--color-dim)]">{person.username}</span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </fieldset>
      {save.isError ? (
        <p role="alert" className="mt-3 text-sm text-[var(--color-text)]">
          {save.error.message}
        </p>
      ) : null}
      <div className="mt-6 flex justify-end gap-2">
        <button type="button" onClick={onClose} className={BUTTON}>
          {strings.folders.cancel}
        </button>
        <button
          type="button"
          disabled={save.isPending}
          onClick={() => save.mutate()}
          className={`${BUTTON} font-semibold text-[var(--color-bright)]`}
        >
          {save.isPending ? strings.sharing.saving : strings.sharing.save}
        </button>
      </div>
    </Dialog>
  )
}
