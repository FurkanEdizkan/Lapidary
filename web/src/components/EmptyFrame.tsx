/**
 * What stands in for a missing picture (A29): an empty frame, the sign-in wall's empty tile, on the lamp the well
 * already draws. Never moving, and decorative: the caller says "no picture" to a reader.
 */
export function EmptyFrame({ className }: { className?: string }) {
  return (
    <div
      aria-hidden="true"
      data-empty-frame
      className={`aspect-square rounded-[var(--radius-md)] border border-[var(--color-border)] ${className ?? ''}`}
    />
  )
}
