/**
 * Shapes of a page while its data is on the way (A5), in the style of `GridSkeleton`: the page has its
 * layout before it has its content, and nothing jumps when the content lands. Still, not pulsing:
 * motion here is for things a person did, and waiting is not one. The sentence is for a screen reader;
 * the blocks say it to everyone else.
 */

const BAR = 'rounded-sm bg-[var(--color-border)]'

/** A part's page: the stage on the left, the name, buttons and figures on the right. */
export function PartSkeleton({ label }: { label: string }) {
  return (
    <div role="status" className="mt-4">
      <span className="sr-only">{label}</span>
      <div aria-hidden="true" className="grid gap-6 md:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="aspect-[4/3] rounded-md border border-[var(--color-border)] bg-[var(--color-raised)]" />
        <div className="flex flex-col gap-3">
          <div className={`h-6 w-4/5 ${BAR}`} />
          <div className="flex gap-2">
            <div className={`h-8 w-24 ${BAR}`} />
            <div className={`h-8 w-24 ${BAR}`} />
          </div>
          {Array.from({ length: 6 }, (_, index) => (
            <div key={index} className="flex gap-4">
              <div className={`h-3 w-24 ${BAR}`} />
              <div className={`h-3 w-32 ${BAR}`} />
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

/** One dashboard tile's body. */
export function TileSkeleton({ label }: { label: string }) {
  return (
    <div role="status" className="flex flex-col gap-2">
      <span className="sr-only">{label}</span>
      <div aria-hidden="true" className={`h-6 w-1/3 ${BAR}`} />
      <div aria-hidden="true" className={`h-3 w-2/3 ${BAR}`} />
      <div aria-hidden="true" className={`h-3 w-1/2 ${BAR}`} />
    </div>
  )
}

/** The whole app's shape, header and grid, while the app itself moves to another port. */
export function AppSkeleton({ label }: { label: string }) {
  return (
    <div role="status" className="min-h-screen bg-[var(--color-bg)]">
      <span className="sr-only">{label}</span>
      <div aria-hidden="true" className="flex h-14 items-center gap-4 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-4">
        <div className={`h-4 w-28 ${BAR}`} />
        <div className={`h-8 flex-1 ${BAR}`} />
        <div className={`h-8 w-8 ${BAR}`} />
      </div>
      <div aria-hidden="true" className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 p-4">
        {Array.from({ length: 12 }, (_, index) => (
          <div key={index} className="aspect-[3/4] rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]" />
        ))}
      </div>
    </div>
  )
}
