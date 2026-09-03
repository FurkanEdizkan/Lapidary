/**
 * Every user-facing string. English only; Turkish is the planned second locale, which
 * is why nothing is inlined in a component.
 */
export const strings = {
  appName: 'Lapidary',
  health: {
    checking: 'Checking the server…',
    ok: (major: number) => `Connected — PostgreSQL ${major}`,
    failed: 'Could not reach the server. Check that the api and db services are running.',
  },
  parts: {
    loading: 'Loading parts…',
    failed: 'Could not load the parts in this library. Check that the api service is running, then reload.',
    thumbnailAlt: (name: string) => `Rendered preview of ${name}`,
    noThumbnail: 'No preview yet',
    triangles: (count: number) =>
      count === 1 ? '1 triangle' : `${count.toLocaleString('en-US')} triangles`,
    /**
     * Shown whenever `PartCard.approximate` is set. CLAUDE.md makes this
     * non-negotiable: mesh-derived measurements are labelled approximate in the UI,
     * always. The wording stays as weak as the flag it renders — the flag means *any*
     * figure on the part is tessellation-derived, not every one — so the badge names
     * the part, and the detail says which figures it can be speaking about.
     */
    approximate: 'Approximate',
    approximateDetail:
      'At least one figure on this part is measured from tessellated geometry rather than from analytic CAD entities.',
    /**
     * The whole library fitted in one page, so the count is the count.
     */
    showingAll: (count: number) =>
      count === 1 ? 'Showing 1 part.' : `Showing all ${count.toLocaleString('en-US')} parts.`,
    /**
     * The server capped the page and there is more behind it. The grid asks for one
     * page and renders it — paging and virtualization are a later slice — so a library
     * larger than a page is genuinely truncated on screen, and saying so is the whole
     * point of this string. A grid that silently shows the first 50 of 200 parts is a
     * measurement that lies by omission.
     */
    showingFirstPage: (count: number) =>
      `Showing the first ${count.toLocaleString('en-US')} parts. This library has more — paging through them arrives with the virtualized grid.`,
  },
  emptyLibrary: {
    title: 'Nothing scanned yet',
    body:
      'This library is empty. Lapidary ingests from a directory mounted on the server, not from this page — run a scan against that directory and every model it finds appears here.',
  },
} as const
