import type { LibraryId, RelatedTags, TagCount, TagIndex } from './types'
import { RefusedError, refusalReason } from './api'

/**
 * The tag index's two reads, and the sorting the pages do with the answer.
 *
 * Its own module rather than more of `lib/api.ts`, for the reason `lib/likeness.ts` and
 * `lib/dashboard.ts` are: one feature, one vocabulary. **These are not the facet.**
 * `fetchFacets` counts within the grid a person has narrowed and withholds its counts past the
 * server's exact-count threshold (`docs/DATA.md` §3.4); an index of a library's tags wants the whole
 * library and cannot drop the counts, because the count is what it is ordered by. So there are two
 * reads, and this is the one that answers "what tags does this library have".
 *
 * Both orders are done here rather than on the server. The whole list arrives in one response — it
 * has to, since every tag must be reachable from `/tags` — so sorting it again costs nothing and a
 * person flipping between "most used" and "A to Z" makes no request.
 */

/**
 * Every tag in a library, with how many live parts carry it.
 *
 * The 200 is checked for a list, the guard `fetchFolds` and `resolve` both make: this page is the
 * only route to a tag that is not already on a part in front of you, and `.map` over an object takes
 * the whole page down rather than one row of it.
 */
export async function fetchTagIndex(library: LibraryId): Promise<TagCount[]> {
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/tags`)
  if (!response.ok) {
    throw new RefusedError(`tag index returned ${response.status}`, await refusalReason(response))
  }
  const body = (await response.json()) as TagIndex
  if (!Array.isArray(body.tags)) {
    throw new RefusedError('tag index answered without a list of tags', undefined)
  }
  return body.tags
}

/**
 * One tag: how many live parts carry it, and the tags most often on those same parts.
 *
 * `parts: 0` is the answer for a tag no live part carries any more, which is a page that has to say
 * so rather than draw an empty grid and leave a person wondering. The tag rides as `?tag=` and not
 * as a path segment, because a tag may hold a slash.
 */
export async function fetchRelatedTags(library: LibraryId, tag: string): Promise<RelatedTags> {
  const query = new URLSearchParams({ tag })
  const response = await fetch(`/api/libraries/${encodeURIComponent(library)}/tags/related?${query}`)
  if (!response.ok) {
    throw new RefusedError(`related tags returned ${response.status}`, await refusalReason(response))
  }
  const body = (await response.json()) as RelatedTags
  if (!Array.isArray(body.related)) {
    throw new RefusedError('related tags answered without a list', undefined)
  }
  return body
}

/**
 * Most-carried first, then alphabetically — the order the server already sends, restated here so the
 * page does not depend on it. A count order with no tie-break puts two equal tags in whichever order
 * the query planner felt like, and a list that reorders itself between loads is a list nobody trusts.
 */
export function byCount(tags: readonly TagCount[]): TagCount[] {
  return [...tags].sort((a, b) => b.count - a.count || compare(a.value, b.value))
}

/** A to Z, as a reader of the language orders it — `localeCompare`, so `Éclair` is not after `Zulu`. */
export function byName(tags: readonly TagCount[]): TagCount[] {
  return [...tags].sort((a, b) => compare(a.value, b.value))
}

function compare(a: string, b: string): number {
  return a.localeCompare(b, 'en', { numeric: true, sensitivity: 'base' })
}

/**
 * The letter a tag is filed under: its first letter, uppercased. Anything that does not start with a
 * letter — `28 mm`, `3d-print`, `#terrain` — is filed together under one heading, because a jump bar
 * with a row for `2`, one for `3` and one for `#` is a jump bar nobody uses.
 *
 * `toLocaleUpperCase` and not `toUpperCase`: the planned second locale is Turkish, where `i`
 * uppercases to `İ`, and a heading that disagrees with the jump link above it is a link that goes
 * nowhere.
 */
export const OTHER_INITIAL = '#'

export function initialOf(tag: string): string {
  const first = [...tag.trim()][0] ?? ''
  const upper = first.toLocaleUpperCase('en')
  return /\p{L}/u.test(upper) ? upper : OTHER_INITIAL
}

/**
 * The name-ordered list cut into letter groups, in the order the groups appear. Built from the sorted
 * list rather than from an alphabet, so a letter no tag starts with gets no heading and no dead link.
 */
export function byInitial(tags: readonly TagCount[]): { initial: string; tags: TagCount[] }[] {
  const groups: { initial: string; tags: TagCount[] }[] = []
  for (const tag of byName(tags)) {
    const initial = initialOf(tag.value)
    const last = groups.at(-1)
    if (last?.initial === initial) last.tags.push(tag)
    else groups.push({ initial, tags: [tag] })
  }
  return groups
}

/**
 * Past this many tags the list gets a letter jump. Below it the whole list is a screen or two and a
 * jump bar is furniture; a creator library of a few hundred tags is where scrolling stops working.
 */
export const LETTER_JUMP_AT = 40

/** The anchor a letter's heading carries and its jump link points at. */
export function letterId(initial: string): string {
  return `tags-${initial === OTHER_INITIAL ? 'other' : initial.toLowerCase()}`
}
