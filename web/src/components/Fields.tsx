import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { createField, fetchFields, removeField, updateField, type FieldWritten } from '../lib/api'
import { strings } from '../lib/strings'
import type { CustomField, FieldFacet, FieldKind, LibraryId } from '../lib/types'
import { Dialog } from './Dialog'

/**
 * A key proposed from a label: lowercase, Turkish and accented letters folded to plain ones, anything
 * else an underscore. Only a proposal. The key cannot be renamed later, so the person who types the
 * label is the one who decides it.
 */
export function proposeKey(label: string): string {
  return label
    .toLowerCase()
    .replace(/ı/g, 'i')
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 40)
}

const KINDS: readonly FieldKind[] = ['text', 'number', 'choice']

/** Each kind's label. A lookup rather than a ternary in JSX, for the reason `LAYOUT_LABEL` gives. */
const KIND_LABEL: Record<FieldKind, string> = {
  text: strings.fields.text,
  number: strings.fields.number,
  choice: strings.fields.choice,
}

/** A choice's options as typed, one per line, blanks dropped. The server refuses a repeat. */
function lines(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

const CONTROL =
  'mt-0.5 block w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 text-sm'
const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/** The library menu's entry for its fields: the button, and the dialog it opens. */
export function FieldsMenuItem({ library }: { library: LibraryId }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.fields.menu}
      </button>
      {open ? <FieldsDialog library={library} onClose={() => setOpen(false)} /> : null}
    </>
  )
}

/**
 * A library's fields: each one relabelled, its options changed, offered as a filter or not, or removed,
 * and a new one added. Every refusal is the server's own sentence, shown under the list.
 */
function FieldsDialog({ library, onClose }: { library: LibraryId; onClose: () => void }) {
  const queryClient = useQueryClient()
  const fields = useQuery({ queryKey: ['fields', library], queryFn: () => fetchFields(library) })
  const [note, setNote] = useState<string | null>(null)
  const settle = (result: FieldWritten): boolean => {
    if (result.kind === 'refused') {
      setNote(result.message)
      return false
    }
    setNote(null)
    void queryClient.invalidateQueries({ queryKey: ['fields', library] })
    return true
  }
  const none = fields.data !== undefined && fields.data.length === 0
  return (
    <Dialog title={strings.fields.dialogTitle} onClose={onClose}>
      {fields.isError ? (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {strings.fields.loadFailed}
        </p>
      ) : null}
      {none ? <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.fields.none}</p> : null}
      {fields.data === undefined || none ? null : (
        <ul role="list" className="mt-3 flex list-none flex-col gap-3">
          {fields.data.map((field) => (
            <FieldRow key={field.key} library={library} field={field} settle={settle} />
          ))}
        </ul>
      )}
      <NewField library={library} settle={settle} />
      {note === null ? null : (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {note}
        </p>
      )}
      <p className="mt-3 text-xs text-[var(--color-muted)]">{strings.fields.removeNote}</p>
      <div className="mt-4 flex justify-end">
        <button type="button" onClick={onClose} className={BUTTON}>
          {strings.fields.close}
        </button>
      </div>
    </Dialog>
  )
}

function FieldRow({
  library,
  field,
  settle,
}: {
  library: LibraryId
  field: CustomField
  settle: (result: FieldWritten) => boolean
}) {
  const [label, setLabel] = useState(field.label)
  const [options, setOptions] = useState(field.options.join('\n'))
  const choice = field.kind === 'choice'
  const save = useMutation({
    mutationFn: () =>
      updateField(library, field.key, { label, ...(choice ? { options: lines(options) } : {}) }),
    onSuccess: settle,
  })
  const offer = useMutation({
    mutationFn: (indexed: boolean) => updateField(library, field.key, { indexed }),
    onSuccess: settle,
  })
  const remove = useMutation({
    mutationFn: () => removeField(library, field.key),
    onSuccess: settle,
  })
  const busy = save.isPending || offer.isPending || remove.isPending
  return (
    <li className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] p-2">
      <div className="flex items-baseline justify-between gap-2 text-xs text-[var(--color-muted)]">
        <span className="font-mono">{field.key}</span>
        <span>{KIND_LABEL[field.kind]}</span>
      </div>
      <form
        className="mt-1 flex flex-col gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          if (!busy) save.mutate()
        }}
      >
        <label className="text-xs text-[var(--color-muted)]">
          {strings.fields.label}
          <input value={label} onChange={(event) => setLabel(event.target.value)} className={CONTROL} />
        </label>
        {choice ? (
          <label className="text-xs text-[var(--color-muted)]">
            {strings.fields.options}
            <textarea
              rows={3}
              value={options}
              onChange={(event) => setOptions(event.target.value)}
              className={CONTROL}
            />
          </label>
        ) : null}
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={field.indexed}
            disabled={busy}
            onChange={(event) => offer.mutate(event.target.checked)}
            className="accent-[var(--color-accent)]"
          />
          {strings.fields.offered}
        </label>
        <div className="flex justify-end gap-2">
          <button type="button" disabled={busy} onClick={() => remove.mutate()} className={BUTTON}>
            {strings.fields.remove(field.label)}
          </button>
          <button type="submit" disabled={busy} className={BUTTON}>
            {save.isPending ? strings.fields.saving : strings.fields.save}
          </button>
        </div>
      </form>
    </li>
  )
}

function NewField({
  library,
  settle,
}: {
  library: LibraryId
  settle: (result: FieldWritten) => boolean
}) {
  const [label, setLabel] = useState('')
  const [key, setKey] = useState('')
  const [keyTyped, setKeyTyped] = useState(false)
  const [kind, setKind] = useState<FieldKind>('text')
  const [options, setOptions] = useState('')
  const [indexed, setIndexed] = useState(false)
  const choice = kind === 'choice'
  const create = useMutation({
    mutationFn: () =>
      createField(library, {
        key,
        label: label.trim(),
        kind,
        ...(choice ? { options: lines(options) } : {}),
        indexed,
      }),
    onSuccess: (result) => {
      if (!settle(result)) return
      setLabel('')
      setKey('')
      setKeyTyped(false)
      setKind('text')
      setOptions('')
      setIndexed(false)
    },
  })
  const incomplete = label.trim().length === 0 || key.length === 0
  return (
    <form
      className="mt-4 flex flex-col gap-2 border-t border-[var(--color-edge)] pt-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (!incomplete && !create.isPending) create.mutate()
      }}
    >
      <label className="text-xs text-[var(--color-muted)]">
        {strings.fields.label}
        <input
          value={label}
          onChange={(event) => {
            setLabel(event.target.value)
            if (!keyTyped) setKey(proposeKey(event.target.value))
          }}
          className={CONTROL}
        />
      </label>
      <label className="text-xs text-[var(--color-muted)]">
        {strings.fields.key}
        <input
          value={key}
          onChange={(event) => {
            setKey(event.target.value)
            setKeyTyped(true)
          }}
          className={`${CONTROL} font-mono`}
        />
        <span className="mt-0.5 block">{strings.fields.keyDetail}</span>
      </label>
      <label className="text-xs text-[var(--color-muted)]">
        {strings.fields.kind}
        <select
          value={kind}
          onChange={(event) => setKind(event.target.value as FieldKind)}
          className={CONTROL}
        >
          {KINDS.map((option) => (
            <option key={option} value={option}>
              {KIND_LABEL[option]}
            </option>
          ))}
        </select>
      </label>
      {choice ? (
        <label className="text-xs text-[var(--color-muted)]">
          {strings.fields.options}
          <textarea
            rows={3}
            value={options}
            onChange={(event) => setOptions(event.target.value)}
            className={CONTROL}
          />
        </label>
      ) : null}
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={indexed}
          onChange={(event) => setIndexed(event.target.checked)}
          className="accent-[var(--color-accent)]"
        />
        {strings.fields.offered}
      </label>
      <div className="flex justify-end">
        <button type="submit" disabled={incomplete || create.isPending} className={BUTTON}>
          {create.isPending ? strings.fields.saving : strings.fields.add}
        </button>
      </div>
    </form>
  )
}

/** Writes a field filter: a value, or for a number a range with either bound or both. `null` clears it. */
type FieldSelect = (field: string | null, value: string | null, range?: { min?: string; max?: string }) => void

/** A filter's typed box, beside the facets. */
const FILTER_BOX =
  'min-w-0 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-0.5 text-sm'

/**
 * The grid's filters for the fields its library offers as filters, beside the facets. One field at a
 * time, as there is one material and one tag: a choice lists its options, text takes a typed value, and a
 * number a range from one box to the other.
 */
export function FieldFilters({
  library,
  field,
  fieldValue,
  fieldMin,
  fieldMax,
  counts,
  onSelect,
}: {
  library: LibraryId
  field?: string
  fieldValue?: string
  fieldMin?: string
  fieldMax?: string
  /** How many of the grid's parts hold each option of each choice field, once the facets have come back. */
  counts?: readonly FieldFacet[]
  onSelect: FieldSelect
}) {
  const fields = useQuery({ queryKey: ['fields', library], queryFn: () => fetchFields(library) })
  const offered = (fields.data ?? []).filter((one) => one.indexed)
  const ranged = fieldMin !== undefined || fieldMax !== undefined
  return (
    <>
      {offered.map((one) => (
        <FieldFilter
          // Keyed by the value and range in force too, so the boxes start again from them whenever the URL
          // changes.
          key={`${one.key}:${field === one.key ? `${fieldValue ?? ''}:${fieldMin ?? ''}:${fieldMax ?? ''}` : ''}`}
          field={one}
          active={field === one.key ? fieldValue : undefined}
          range={field === one.key && ranged ? { min: fieldMin, max: fieldMax } : undefined}
          counts={counts?.find((counted) => counted.key === one.key)?.values}
          onSelect={onSelect}
        />
      ))}
    </>
  )
}

function FieldFilter({
  field,
  active,
  range,
  counts,
  onSelect,
}: {
  field: CustomField
  active?: string
  /** The range in force on this number field, when one is. */
  range?: { min?: string; max?: string }
  /** How many of the grid's parts hold each option, for a choice. Absent until the facets come back. */
  counts?: FieldFacet['values']
  onSelect: FieldSelect
}) {
  const [draft, setDraft] = useState(active ?? '')
  // A value from an older link starts both ends at it, which filters the same parts.
  const [from, setFrom] = useState(range?.min ?? active ?? '')
  const [to, setTo] = useState(range?.max ?? active ?? '')
  const id = `field-filter-${field.key}`
  const choice = field.kind === 'choice'
  const numeric = field.kind === 'number'
  // An option no part holds is left out of the counts, and holds none. Past the server's threshold every count
  // is withheld, and that none with them, or the options no part holds would be the only ones with a number.
  const countOf = (option: string): number | null => {
    if (counts === undefined) return null
    const counted = counts.find(({ value }) => value === option)
    if (counted !== undefined) return counted.count
    return counts.some(({ count }) => count === null) ? null : 0
  }
  return (
    <section aria-labelledby={id} className="mb-6">
      <h2 id={id} className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {field.label}
      </h2>
      {choice ? (
        <ul role="list" className="flex list-none flex-col gap-0.5">
          {field.options.map((option) => {
            const count = countOf(option)
            return (
              <li key={option}>
                <button
                  type="button"
                  aria-pressed={active === option}
                  aria-label={strings.fields.choiceOption(option, count)}
                  onClick={() =>
                    active === option ? onSelect(null, null) : onSelect(field.key, option)
                  }
                  className="ease-mechanical flex w-full items-center justify-between gap-2 rounded-sm px-2 py-0.5 text-left text-sm duration-[var(--duration-fast)] hover:bg-[var(--color-surface)] aria-pressed:bg-[var(--color-surface)] aria-pressed:text-[var(--color-bright)]"
                >
                  <span>{option}</span>
                  {count === null ? null : (
                    <span className="tabular text-xs text-[var(--color-muted)]">{strings.facets.count(count)}</span>
                  )}
                </button>
              </li>
            )
          })}
        </ul>
      ) : numeric ? (
        <form
          className="flex gap-1"
          onSubmit={(event) => {
            event.preventDefault()
            // A comma is taken for the point, as the Densities dialog takes it.
            const typed = [from.trim().replace(',', '.'), to.trim().replace(',', '.')] as const
            // Typed backwards, meant forwards: 22 to 8 is 8 to 22, and the boxes come back that way round.
            const backwards =
              typed.every((end) => end.length > 0 && Number.isFinite(Number(end))) && Number(typed[0]) > Number(typed[1])
            const [min, max] = backwards ? [typed[1], typed[0]] : typed
            if (min.length === 0 && max.length === 0) onSelect(null, null)
            else onSelect(field.key, null, { min: min.length === 0 ? undefined : min, max: max.length === 0 ? undefined : max })
          }}
        >
          <input
            aria-label={strings.fields.filterFrom(field.label)}
            placeholder={strings.fields.from}
            inputMode="decimal"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
            className={FILTER_BOX}
          />
          <input
            aria-label={strings.fields.filterTo(field.label)}
            placeholder={strings.fields.to}
            inputMode="decimal"
            value={to}
            onChange={(event) => setTo(event.target.value)}
            className={FILTER_BOX}
          />
          <button type="submit" className={BUTTON}>
            {strings.fields.filterApply}
          </button>
        </form>
      ) : (
        <form
          className="flex gap-1"
          onSubmit={(event) => {
            event.preventDefault()
            const value = draft.trim()
            if (value.length === 0) onSelect(null, null)
            else onSelect(field.key, value)
          }}
        >
          <input
            aria-label={strings.fields.filterValue(field.label)}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            className={FILTER_BOX}
          />
          <button type="submit" className={BUTTON}>
            {strings.fields.filterApply}
          </button>
        </form>
      )}
      {active === undefined && range === undefined ? null : (
        <button
          type="button"
          onClick={() => onSelect(null, null)}
          className="mt-1 text-xs text-[var(--color-muted)] underline"
        >
          {strings.fields.filterClear(field.label)}
        </button>
      )}
    </section>
  )
}
