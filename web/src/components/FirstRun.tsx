import { useId } from 'react'
import { strings } from '../lib/strings'
import { Icon } from './Icon'
import { QUIET, STANDING } from './Page'

/**
 * An empty library, which is what every library somebody creates starts as: what it is for, and
 * the one way in that a click reaches.
 *
 * **The turning bench is gone** (R1). It was three bundled rungs on a canvas, which meant the
 * renderer and `turntable.ts` were loaded by the one page in the application that has nothing to
 * render — and nothing produces rungs any more. The heading, the sentence and Upload were always
 * the page; the scene said nothing a reader needed, which is why it was `aria-hidden`.
 */
export function FirstRun({ onImport, onUpload }: { onImport: () => void; onUpload: () => void }) {
  const titleId = useId()

  return (
    <section aria-labelledby={titleId} className="py-4 lg:py-10">
      <div className="max-w-prose">
        <h2 id={titleId} className="text-3xl leading-tight font-semibold tracking-tight text-[var(--color-bright)]">
          {strings.emptyLibrary.title}
        </h2>
        <p className="mt-3 text-[var(--color-dim)]">{strings.emptyLibrary.body}</p>
        {/*
          The lead's first verb stands (A17's audit): Import, which reads the models folder in place. Upload a folder
          is the quiet second way, and the drop the third, said under both.
        */}
        <div className="mt-6 flex flex-wrap items-center gap-3">
          <button type="button" onClick={onImport} className={`whitespace-nowrap ${STANDING}`}>
            {strings.addModels.button}
          </button>
          <button type="button" onClick={onUpload} className={QUIET}>
            <Icon name="upload" />
            {strings.toolbar.uploadFolder}
          </button>
        </div>
        <p className="mt-3 text-xs text-[var(--color-muted)]">{strings.emptyLibrary.dropHint}</p>
      </div>
    </section>
  )
}
