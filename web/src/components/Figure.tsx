import { strings } from '../lib/strings'
import type { Approximate } from '../lib/types'

/**
 * A measured value and its provenance, which cannot be rendered apart.
 *
 * The badge is on the figure rather than on the page because provenance is per figure:
 * `Approximate<T>` carries the flag with the value precisely so a caller cannot show one
 * without the other, and this is that type reaching the screen.
 */
export function Figure<T>({
  figure,
  render,
}: {
  figure: Approximate<T>
  render: (value: T) => string
}) {
  return (
    <span
      title={figure.approximate ? strings.detail.approximateTitle : strings.detail.exactTitle}
    >
      {render(figure.value)}
      {figure.approximate ? (
        <>
          <span aria-hidden="true" className="ml-1 text-[var(--color-muted)]">
            {strings.detail.approximate}
          </span>
          <span className="sr-only">{strings.detail.approximateSpoken}</span>
        </>
      ) : null}
    </span>
  )
}
