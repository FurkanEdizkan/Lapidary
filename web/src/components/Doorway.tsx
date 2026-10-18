import type { ReactNode } from 'react'
import { strings } from '../lib/strings'
import { BrandMark } from './AppFrame'

/**
 * The frame for the two pages a person sees before they are signed in: setup and sign-in (A1). No
 * header, no places and no menu, because none of them work yet — the mark, then one narrow column.
 *
 * With an `aside` (A19) it splits: at `lg` the aside stands on the lamp in the wider left column with the mark, the
 * wordmark and the pitch at its foot, and the page's own content is centred on the right. Below `lg` the aside is a
 * short band above the content. `done` fills the mark's diamond: setup's last moment (A20). A `backdrop` (A28, the
 * sign-in wall) splits it the same way but fills the whole left column or band, under the mark.
 */
export function Doorway({
  children,
  aside,
  backdrop,
  done = false,
}: {
  children: ReactNode
  aside?: ReactNode
  backdrop?: ReactNode
  done?: boolean
}) {
  if (aside !== undefined || backdrop !== undefined) {
    return (
      <main className="grid min-h-dvh grid-rows-[auto_1fr] lg:grid-cols-[55fr_45fr] lg:grid-rows-1">
        <div className="stage-lamp relative flex h-36 flex-col overflow-hidden border-b border-[var(--color-border)] lg:h-auto lg:border-r lg:border-b-0">
          {/*
            Below `lg` the drawing is larger than its band and cropped by it, off to the right of the mark. Centred by
            flex, not a translate: a transformed ancestor would capture setup's intro, which centres on the viewport.
          */}
          {backdrop}
          {aside === undefined ? null : (
            <div className="absolute inset-y-0 -right-14 flex w-80 items-center lg:static lg:min-h-0 lg:w-auto lg:flex-1 lg:justify-center lg:p-16">
              {aside}
            </div>
          )}
          <div className="absolute bottom-0 left-0 z-10 flex flex-col gap-3 p-4 lg:relative lg:mt-auto lg:p-12">
            <div className="flex items-center gap-[9px]">
              <BrandMark filled={done} />
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
      {hint === undefined ? null : <span className="max-w-[60ch] text-xs text-[var(--color-muted)]">{hint}</span>}
    </label>
  )
}

export const INPUT =
  'rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm text-[var(--color-text)]'
