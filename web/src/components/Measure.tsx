import { TOOLS, type Tool } from '../lib/measure'
import { strings } from '../lib/strings'
import type { Approximate } from '../lib/types'
import { Figure } from './Figure'

/**
 * The measuring tools under the 3D view, and the line saying what to click or what was measured.
 *
 * A component of its own, outside the three.js chunk, so `Measure.test.tsx` can render the value
 * line in jsdom: it is where a measurement meets the screen, and `Figure` is what makes an
 * approximate one say so.
 */
export function MeasureBar({
  tool,
  onTool,
  reading,
  note,
}: {
  tool: Tool | null
  /** Pressing the tool that is on turns it off. */
  onTool: (tool: Tool | null) => void
  reading: Approximate<number> | null
  /** What to do next, shown while there is no reading. */
  note: string | null
}) {
  return (
    <div className="mt-2">
      <div role="toolbar" aria-label={strings.measure.label} className="flex flex-wrap gap-1">
        {TOOLS.map((option) => (
          <button
            key={option}
            type="button"
            aria-pressed={tool === option}
            onClick={() => onTool(tool === option ? null : option)}
            className="ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] aria-pressed:border-[var(--color-accent)] aria-pressed:text-[var(--color-bright)]"
          >
            {strings.measure.tools[option]}
          </button>
        ))}
      </div>
      {tool === null ? null : (
        <p aria-live="polite" className="mt-2 text-sm">
          {reading === null ? (
            <span className="text-[var(--color-muted)]">{note}</span>
          ) : (
            <>
              <span className="mr-2 text-[var(--color-muted)]">{strings.measure.tools[tool]}</span>
              <Figure
                figure={reading}
                render={tool === 'angle' ? strings.measure.degrees : strings.measure.millimetres}
              />
            </>
          )}
        </p>
      )}
    </div>
  )
}
