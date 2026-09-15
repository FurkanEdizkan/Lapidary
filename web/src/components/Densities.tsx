import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { fetchDensities, fetchFacets, removeDensity, setDensity, type FieldWritten } from '../lib/api'
import { strings } from '../lib/strings'
import type { LibraryId } from '../lib/types'
import { Dialog, closeMenu } from './Dialog'

const CONTROL =
  'mt-0.5 block w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 text-sm'
const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/** A density typed in g/cm³, a comma allowed for the point, as the kg/m³ it is stored in; `null` for anything else. */
export function kgPerM3(typed: string): number | null {
  const text = typed.trim().replace(',', '.')
  const value = Number(text)
  return text === '' || !Number.isFinite(value) ? null : Math.round(value * 1_000_000) / 1000
}

/** A stored kg/m³ as the g/cm³ a person types. */
export function gramsPerCm3(kg: number): string {
  return String(Number((kg / 1000).toFixed(4)))
}

/** The library menu's entry for its densities: the button, and the dialog it opens. */
export function DensitiesMenuItem({ library }: { library: LibraryId }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        type="button"
        onClick={(event) => {
          closeMenu(event.currentTarget)
          setOpen(true)
        }}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.densities.menu}
      </button>
      {open ? <DensitiesDialog library={library} onClose={() => setOpen(false)} /> : null}
    </>
  )
}

/**
 * A library's densities: a box for each material its parts hold, and for each material that already has a
 * density though no part holds it now. Typed and shown in g/cm³, stored in kg/m³. Every refusal is the
 * server's own sentence, shown under the list, but for a density out of range, which the server says in kg/m³.
 */
function DensitiesDialog({ library, onClose }: { library: LibraryId; onClose: () => void }) {
  const queryClient = useQueryClient()
  const densities = useQuery({ queryKey: ['densities', library], queryFn: () => fetchDensities(library) })
  const facets = useQuery({ queryKey: ['facets', library, 'densities'], queryFn: () => fetchFacets(library) })
  const [note, setNote] = useState<string | null>(null)
  const settle = (result: FieldWritten): boolean => {
    if (result.kind === 'refused') {
      setNote(result.reason === 'badDensity' ? strings.densities.outOfRange : result.message)
      return false
    }
    setNote(null)
    void queryClient.invalidateQueries({ queryKey: ['densities', library] })
    // A mass is worked out from its density when read, so an open part's history and comparison read again.
    void queryClient.invalidateQueries({ queryKey: ['revisions'] })
    void queryClient.invalidateQueries({ queryKey: ['diff'] })
    return true
  }
  const loaded = densities.data !== undefined && facets.data !== undefined
  const materials = [
    ...new Set([
      ...(facets.data?.materials ?? []).map((material) => material.value),
      ...(densities.data ?? []).map((density) => density.material),
    ]),
  ].sort((a, b) => a.localeCompare(b))
  return (
    <Dialog title={strings.densities.dialogTitle} onClose={onClose}>
      <p className="mt-2 text-xs text-[var(--color-muted)]">{strings.densities.intro}</p>
      {densities.isError || facets.isError ? (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {strings.densities.loadFailed}
        </p>
      ) : null}
      {loaded && materials.length === 0 ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.densities.none}</p>
      ) : null}
      {!loaded || materials.length === 0 ? null : (
        <ul role="list" className="mt-3 flex list-none flex-col gap-2">
          {materials.map((material) => (
            <DensityRow
              key={material}
              library={library}
              material={material}
              density={densities.data?.find((density) => density.material === material)?.densityKgM3 ?? null}
              settle={settle}
            />
          ))}
        </ul>
      )}
      {note === null ? null : (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {note}
        </p>
      )}
      <div className="mt-4 flex justify-end">
        <button type="button" onClick={onClose} className={BUTTON}>
          {strings.densities.close}
        </button>
      </div>
    </Dialog>
  )
}

function DensityRow({
  library,
  material,
  density,
  settle,
}: {
  library: LibraryId
  material: string
  density: number | null
  settle: (result: FieldWritten) => boolean
}) {
  const [typed, setTyped] = useState(density === null ? '' : gramsPerCm3(density))
  const [unreadable, setUnreadable] = useState(false)
  const save = useMutation({
    mutationFn: (kg: number) => setDensity(library, material, kg),
    onSuccess: settle,
  })
  const remove = useMutation({
    mutationFn: () => removeDensity(library, material),
    onSuccess: (result) => {
      if (settle(result)) setTyped('')
    },
  })
  const busy = save.isPending || remove.isPending
  return (
    <li className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] p-2">
      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          const kg = kgPerM3(typed)
          setUnreadable(kg === null)
          if (kg !== null && !busy) save.mutate(kg)
        }}
      >
        <label className="min-w-40 flex-1 text-xs text-[var(--color-muted)]">
          {strings.densities.field(material)}
          <input
            inputMode="decimal"
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            className={CONTROL}
          />
        </label>
        {density === null ? null : (
          <button type="button" disabled={busy} onClick={() => remove.mutate()} className={BUTTON}>
            {strings.densities.remove(material)}
          </button>
        )}
        <button type="submit" disabled={busy} className={BUTTON}>
          {save.isPending ? strings.densities.saving : strings.densities.save}
        </button>
      </form>
      {unreadable ? (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {strings.densities.unreadable}
        </p>
      ) : null}
    </li>
  )
}
