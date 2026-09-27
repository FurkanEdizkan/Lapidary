import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import { Index, useGridWiring, validateGridSearch } from './index'
import { fetchRelatedTags } from '../lib/tags'
import type { LibraryId } from '../lib/types'

/**
 * One tag's page: **the grid, pre-filtered** — the same component, reading the same `tag` the grid's
 * own parameter feeds, with the tags most often on the same models beside it.
 *
 * Not a second grid. `Index` is mounted here exactly as `/` mounts it, wired by the same
 * `useGridWiring`, so the format and material facets, the search box, the category tree, the quick
 * look, the saved filters, upload and scan all work here and go on working when one of them changes.
 * The tag is the one thing the URL holds in its path rather than its search, which is the whole of
 * the difference: `/tags/dragon?format=stl` and `/?tag=dragon&format=stl` show the same models.
 *
 * `tags_.` and not `tags.`: the underscore keeps this out of `/tags`'s layout, so the index of tags
 * is not drawn above every tag's page. `sharing_.shares.$shareId.tsx` is here for the same reason.
 */
export const Route = createFileRoute('/tags_/$tag')({
  component: RouteComponent,
  /*
    The grid's whole search, shared with `/` rather than re-declared: this page reads every one of
    those parameters, and a copy of the parser would be a second answer to "is a tag that is all
    digits a number". `tag` is in it too and is simply not used — a `?tag=` typed onto a tag page is
    the same tag or a contradiction, and the path wins either way.
  */
  validateSearch: validateGridSearch,
})

function RouteComponent() {
  const { tag } = Route.useParams()
  // The tag goes in as the path's, which is what makes every control on the page write back to
  // `/tags/{tag}` instead of to `/`. Everything else is the same wiring `/` uses.
  const wiring = useGridWiring(Route.useSearch(), tag)
  return (
    <>
      {/*
        Rendered here and not inside the grid: React hoists a `<title>` from wherever it is written
        and the document takes the first in tree order, so the page that knows what it is about owns
        it and the grid is told to draw none. `titled={false}` is that instruction.
      */}
      <title>{strings.tagIndex.tagTitle(tag)}</title>
      <Index
        {...wiring}
        titled={false}
        heading={tag}
        intro={<Related library={wiring.library} tag={tag} />}
      />
    </>
  )
}

/**
 * What else is on these models, and — when nothing is — why the grid below is empty.
 *
 * Its own request, not the grid's. The grid answers "which models carry this tag"; this answers
 * "what else is on them", which is a different question with a different answer, and a page that
 * asked for both in one read would make the models wait for the neighbours. It asks once, on arrival:
 * there is no timer here, as there is none on the dashboard.
 */
function Related({ library, tag }: { library: LibraryId; tag: string }) {
  const near = useQuery({
    queryKey: ['tags-near', library, tag],
    queryFn: () => fetchRelatedTags(library, tag),
  })
  const home = library === DEFAULT_LIBRARY_ID ? {} : { library }
  return (
    <div className="mb-4">
      {near.isError ? (
        /*
          A note and not an alert that takes the page with it: the models below arrived from their own
          request and are right. Only the suggestions are missing.
        */
        <p className="max-w-[70ch] text-xs text-[var(--color-muted)]">
          {strings.tagIndex.relatedFailed}
        </p>
      ) : near.data === undefined ? null : near.data.parts === 0 ? (
        /*
          No live model carries it. The grid below will be empty, and an empty grid on its own reads as
          a filter that went wrong — so the page says which of the two it is. Removal is soft, so the
          honest sentence is about the models, not about a tag being deleted: there is no tag to delete.
        */
        <p role="status" className="max-w-[70ch] text-sm text-[var(--color-muted)]">
          {strings.tagIndex.gone(tag)}
        </p>
      ) : (
        <>
          <p className="text-xs text-[var(--color-muted)]">
            {strings.tagIndex.counted(near.data.parts)}
          </p>
          {near.data.related.length === 0 ? (
            <p className="max-w-[70ch] text-xs text-[var(--color-muted)]">
              {strings.tagIndex.relatedNone}
            </p>
          ) : (
            /*
              Titled, so the chips are not a row of words with no say in what they are. The heading is
              drawn only where there is something under it — over "no model carries this any more" it
              would be a promise of suggestions that are not coming.
            */
            <section aria-labelledby="tag-related" className="mt-2">
              <h3
                id="tag-related"
                className="text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase"
              >
                {strings.tagIndex.related}
              </h3>
              <ul role="list" className="mt-1.5 flex flex-wrap gap-1">
                {near.data.related.map(({ value, count }) => (
                  <li key={value}>
                    {/*
                      No `aria-label`: the accessible name is the tag and its shared count, both of
                      them visible text, which is what WCAG 2.5.3 asks for. The library rides along so
                      a chip in a second library does not open the default library's tag.
                    */}
                    <Link
                      to="/tags/$tag"
                      params={{ tag: value }}
                      search={home}
                      className="ease-mechanical flex min-h-7 items-center gap-2 rounded-sm border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-sm duration-[var(--duration-fast)] hover:-translate-y-px hover:text-[var(--color-bright)]"
                    >
                      <span className="break-words">{value}</span>
                      {/* A space, for the reason `TagRows` gives: the name is the two runs joined. */}
                      {' '}
                      <span className="tabular text-xs text-[var(--color-muted)]">
                        {strings.tagIndex.shared(count)}
                      </span>
                    </Link>
                  </li>
                ))}
              </ul>
              {/* The floor is the server's number, printed rather than restated. */}
              <p className="mt-1.5 max-w-[70ch] text-xs text-[var(--color-muted)]">
                {strings.tagIndex.relatedNote(near.data.floor)}
              </p>
            </section>
          )}
        </>
      )}
      {/*
        The way back, under the suggestions rather than beside their heading: beside it, "Often
        together with" and "All tags" sat a gap apart and read as one phrase. It is drawn whatever the
        panel above says, including while it is still on its way and when the tag is gone — a page
        about a tag nothing carries is exactly where somebody wants the list of the ones that are.
      */}
      <p className="mt-2">
        <Link
          to="/tags"
          search={home}
          className="ease-mechanical text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-bright)]"
        >
          {strings.tagIndex.back}
        </Link>
      </p>
    </div>
  )
}
