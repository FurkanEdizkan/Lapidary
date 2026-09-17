import { useEffect, useId, useRef } from 'react'
import { strings } from '../lib/strings'
import { hasWebGL } from '../lib/viewer-math'
import { Icon } from './Icon'

/** The first-run scene's models, the worker's own rungs of three example parts. */
const BENCH = ['/first-run/spur-gear.glb', '/first-run/flange.glb', '/first-run/vee-block.glb']

/**
 * An empty library, which is what every library somebody creates starts as: what it is for, the
 * one way in that a click reaches, and a scene of real parts lit the way the grid will show them.
 *
 * The scene is decoration and says nothing a reader needs, so it is `aria-hidden` and, where the
 * browser cannot draw it, simply absent; the heading, the sentence and Upload are the page.
 */
export function FirstRun({ onUpload }: { onUpload: () => void }) {
  const titleId = useId()
  const host = useRef<HTMLDivElement>(null)
  const drawable = hasWebGL()
  useEffect(() => {
    const node = host.current
    if (!drawable || node === null) return
    let stop: (() => void) | null = null
    let cancelled = false
    void import('./turntable')
      .then((turntable) => {
        if (!cancelled) stop = turntable.bench(node, BENCH)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
      stop?.()
    }
  }, [drawable])

  return (
    <section
      aria-labelledby={titleId}
      className="grid items-center gap-8 py-4 lg:grid-cols-[minmax(0,26rem)_minmax(0,1fr)] lg:py-10"
    >
      <div className="max-w-prose">
        <h2 id={titleId} className="text-3xl leading-tight font-semibold tracking-tight text-[var(--color-bright)]">
          {strings.emptyLibrary.title}
        </h2>
        <p className="mt-3 text-[var(--color-dim)]">{strings.emptyLibrary.body}</p>
        <button
          type="button"
          onClick={onUpload}
          className="ease-mechanical mt-6 flex min-h-9 items-center gap-2 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          <Icon name="upload" />
          {strings.toolbar.upload}
        </button>
        <p className="mt-3 text-xs text-[var(--color-muted)]">{strings.emptyLibrary.dropHint}</p>
      </div>
      {drawable ? (
        <div ref={host} aria-hidden="true" className="stage-lamp relative aspect-[16/10] w-full overflow-hidden rounded-md" />
      ) : null}
    </section>
  )
}
