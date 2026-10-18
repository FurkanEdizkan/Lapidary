import type { ReactNode } from 'react'
import { strings } from '../lib/strings'
import { BrandMark } from './AppFrame'

/**
 * The frame for the two pages a person sees before they are signed in: setup and sign-in (A1). No
 * header, no places and no menu, because none of them work yet — the mark, then one narrow column.
 *
 * With an `aside` (A19) it splits: at `lg` the aside stands on the lamp in the wider left column with the mark, the
 * wordmark and the pitch at its foot, and the page's own content is centred on the right. Below `lg` the aside is a
 * short band above the content.
 */
export function Doorway({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  if (aside !== undefined) {
    return (
      <main className="grid min-h-[100dvh] grid-rows-[auto_1fr] lg:grid-cols-[55fr_45fr] lg:grid-rows-1">
        <div className="stage-lamp relative flex h-[30vh] min-h-56 flex-col overflow-hidden border-b border-[var(--color-border)] lg:h-auto lg:border-r lg:border-b-0">
          <div className="flex min-h-0 flex-1 items-center justify-center px-6 pt-6 lg:px-16 lg:pt-16">{aside}</div>
          <div className="flex flex-col gap-2 px-4 pb-4 lg:px-12 lg:pb-12">
            <div className="flex items-center gap-[9px]">
              <BrandMark />
              <span className="text-xs leading-none font-bold tracking-[0.16em] uppercase">{strings.appName}</span>
            </div>
            <p className="hidden text-sm text-[var(--color-dim)] lg:block">{strings.doorway.pitch}</p>
          </div>
        </div>
        <div className="flex items-start justify-center px-4 pt-10 pb-16 lg:items-center lg:px-12 lg:py-16">
          <div className="w-full max-w-sm">{children}</div>
        </div>
      </main>
    )
  }
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
