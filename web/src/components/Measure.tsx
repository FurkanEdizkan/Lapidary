import { TOOLS, type Tool } from '../lib/measure'
import { strings } from '../lib/strings'
import type { Approximate } from '../lib/types'
import { AXES, type Section } from '../lib/viewer-math'
import { Figure } from './Figure'

const CONTROL =
  'ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] aria-pressed:border-[var(--color-accent)] aria-pressed:text-[var(--color-bright)]'

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
            className={CONTROL}
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

/**
 * The section plane's controls, under the measuring tools: an axis to cut across, where along it,
 * and which side stays. Every change hands back the whole cut, or `null` for none.
 */
export function SectionBar({
  section,
  onSection,
  closed,
}: {
  section: Section | null
  onSection: (section: Section | null) => void
  /** Whether the part is a closed mesh: `null` when nobody measured. Only a closed one's cut is filled. */
  closed: boolean | null
}) {
  // Said while cutting, and only then: an open mesh has no inside to fill, and one nobody measured might
  // not, so neither gets a filled face, and neither is called closed or open when it was not measured.
  const note = section === null || closed === true ? null : closed === false ? strings.section.open : strings.section.unknown
  return (
    <>
    <div role="toolbar" aria-label={strings.section.label} className="mt-2 flex flex-wrap items-center gap-1">
      <button type="button" aria-pressed={section === null} onClick={() => onSection(null)} className={CONTROL}>
        {strings.section.off}
      </button>
      {AXES.map((axis) => (
        <button
          key={axis}
          type="button"
          aria-pressed={section?.axis === axis}
          onClick={() => onSection({ axis, at: section?.at ?? 0.5, flip: section?.flip ?? false })}
          className={CONTROL}
        >
          {strings.section.axes[axis]}
        </button>
      ))}
      {section === null ? null : (
        <>
          <input
            type="range"
            min={0}
            max={1000}
            value={Math.round(section.at * 1000)}
            aria-label={strings.section.position}
            onChange={(event) => onSection({ ...section, at: Number(event.target.value) / 1000 })}
            className="mx-1 min-w-24 flex-1 accent-[var(--color-accent)]"
          />
          <button
            type="button"
            aria-pressed={section.flip}
            onClick={() => onSection({ ...section, flip: !section.flip })}
            className={CONTROL}
          >
            {strings.section.flip}
          </button>
        </>
      )}
    </div>
    {note === null ? null : <p className="mt-1 text-xs text-[var(--color-muted)]">{note}</p>}
    </>
  )
}
