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
  scan: {
    /**
     * Shown while a batch is draining. `done` counts every file the worker has finished
     * with, however it finished — ingested, skipped and failed alike — because what this
     * line answers is "how much is left", and a file that failed is not still pending.
     */
    running: (done: number, total: number) =>
      `Scanning — ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} files.`,
    finished: (ingested: number, skipped: number) =>
      skipped === 0
        ? `Scan complete — ${ingested.toLocaleString('en-US')} added.`
        : `Scan complete — ${ingested.toLocaleString('en-US')} added, ${skipped.toLocaleString('en-US')} already here.`,
    /**
     * A file that will never appear. The count is what belongs on screen; the reason per
     * file is the failed-file drawer, which arrives in Phase 2.
     */
    failed: (count: number) =>
      count === 1
        ? '1 file could not be read. It will not appear in the grid.'
        : `${count.toLocaleString('en-US')} files could not be read. They will not appear in the grid.`,
    /**
     * The batch id in the URL matched nothing this library can show. Deliberately does
     * not distinguish "never issued" from "belongs to another library" — the API does not
     * either, because telling them apart would confirm a batch exists somewhere.
     */
    unknown:
      'No scan with that id has run in this library. It may belong to another library, or have found no files to queue.',
  },
  /**
   * The library-wide settings the action bar can change. One today.
   */
  library: {
    autoThumbnail: 'Render previews automatically as parts are ingested',
    autoThumbnailDetail:
      'Turning this off leaves existing previews alone. Parts ingested afterwards arrive without one until you generate them.',
    autoThumbnailFailed:
      'Could not change this setting. Check that the api service is running, then try again.',
  },
  /**
   * Rendering previews for parts that are already here — the sweep and the per-card
   * action. Deliberately its own copy rather than `scan`'s: a thumbnail batch that
   * finishes ingests nothing and skips nothing, so `scan.finished` would report "Scan
   * complete — 0 added." over 151 successful renders, which reads as a failure.
   */
  render: {
    sweep: 'Generate missing previews',
    part: 'Render preview',
    partFor: (name: string) => `Render the preview for ${name}`,
    /**
     * `queued: 0` from the sweep. A success — every part already has a preview — and
     * the one wording mistake worth guarding against is reporting it as an error.
     */
    nothingMissing: 'Every part in this library already has a preview.',
    queueFailed:
      'Could not queue the preview render. Check that the api service is running, then try again.',
    running: (done: number, total: number) =>
      `Rendering previews — ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')}.`,
    finished: (rendered: number) =>
      rendered === 1
        ? 'Preview rendering complete — 1 preview rendered.'
        : `Preview rendering complete — ${rendered.toLocaleString('en-US')} previews rendered.`,
    /**
     * Named `failed` to match `scan.failed`, so the progress line can pick one copy
     * object by batch kind instead of branching on the kind inside JSX for every field.
     */
    failed: (count: number) =>
      count === 1
        ? '1 preview could not be rendered. That part keeps the preview it had.'
        : `${count.toLocaleString('en-US')} previews could not be rendered. Those parts keep the previews they had.`,
    /**
     * The render batch this page started cannot be read back. Unlike `scan.unknown`
     * this is never a mistyped id — the id came from the `202` — so the advice is to
     * retry rather than to check what was typed.
     */
    unknown:
      'Could not read how the preview rendering is going. The work is queued and continues on the server; reload to pick it up again.',
  },
  emptyLibrary: {
    title: 'Nothing scanned yet',
    body:
      'This library is empty. Lapidary ingests from a directory mounted on the server, not from this page — run a scan against that directory and every model it finds appears here.',
  },
} as const
