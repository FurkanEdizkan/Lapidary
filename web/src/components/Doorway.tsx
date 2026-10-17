import type { ReactNode } from 'react'
import { strings } from '../lib/strings'
import { BrandMark } from './AppFrame'

/**
 * The frame for the two pages a person sees before they are signed in: setup and sign-in (A1). No
 * header, no places and no menu, because none of them work yet — the mark, then one narrow column.
 */
export function Doorway({ children }: { children: ReactNode }) {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-md flex-col px-4 pt-[12vh] pb-16">
      <div className="mb-10 flex items-center gap-[9px]">
        <BrandMark />
        <span className="text-xs leading-none font-bold tracking-[0.16em] uppercase">{strings.appName}</span>
      </div>
      {children}
    </main>
  )
}

/** One labelled field, as both pages draw it. */
export function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <label className="flex flex-col gap-1 text-xs text-[var(--color-muted)]">
      {label}
      {children}
      {hint === undefined ? null : <span className="text-[11px] text-[var(--color-dim)]">{hint}</span>}
    </label>
  )
}

export const INPUT =
  'rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm text-[var(--color-text)]'
