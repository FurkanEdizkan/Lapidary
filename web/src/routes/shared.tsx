import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { AppFrame, type Place } from '../components/AppFrame'
import { Grid, GridSkeleton } from '../components/Grid'
import { HEADLINE, LEAD } from '../components/Page'
import { fetchShared } from '../lib/api'
import { DEFAULT_DENSITY, type Layout } from '../lib/preferences'
import { strings } from '../lib/strings'

export const Route = createFileRoute('/shared')({
  component: SharedPage,
})

const NONE = new Set<never>()
const NOTHING = () => {}
// Out of the JSX, as `AppFrame` keeps its paths: a literal in an attribute reads as copy to the
// bare-strings check.
const LAYOUT: Layout = 'gallery'
const PLACE: Place = 'sharedWithMe'
const EMPTY = 'mt-6 max-w-[70ch] text-sm text-[var(--color-text)]'

/**
 * Shared with me (A4): the models other people shared with this person, newest first. A card opens the
 * model's page, where they can look and download; nothing here changes anything.
 */
export function SharedPage() {
  const navigate = useNavigate()
  const shared = useQuery({ queryKey: ['shared-with-me'], queryFn: fetchShared })
  return (
    <AppFrame current={PLACE}>
      <section>
        <title>{strings.titles.shared}</title>
        <h2 className={HEADLINE}>{strings.sharing.title_page}</h2>
        <p className={LEAD}>{strings.sharing.leadPage}</p>
        {shared.isPending ? (
          <div className="mt-6">
            <GridSkeleton label={strings.sharing.loadingPage} />
          </div>
        ) : shared.isError ? (
          <p role="alert" className={EMPTY}>
            {strings.sharing.failed}
          </p>
        ) : shared.data.parts.length === 0 ? (
          // The page's whole content when nothing is shared, so it reads in Chalk, at the lead's measure.
          <p className={EMPTY}>{strings.sharing.empty}</p>
        ) : (
          <div className="mt-6">
            <Grid
              parts={shared.data.parts}
              density={DEFAULT_DENSITY}
              layout={LAYOUT}
              selecting={false}
              selected={NONE}
              onToggle={NOTHING}
              onSelectAll={NOTHING}
              onOpen={(part) => void navigate({ to: '/parts/$partId', params: { partId: part.id } })}
            />
          </div>
        )}
      </section>
    </AppFrame>
  )
}
