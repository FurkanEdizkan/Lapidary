/**
 * Every user-facing string. English only; Turkish is the planned second locale, which
 * is why nothing is inlined in a component.
 */
/**
 * Decimal, not binary: 1 kB is 1000 B here, which is what `du --si` prints and the unit
 * every figure in the slice handoffs was recorded in. A card reading 91.2 kB beside a
 * shell reporting 89 KiB is a disagreement nobody can resolve without first knowing
 * which convention each side picked.
 */
const BYTE_UNITS = ['B', 'kB', 'MB', 'GB', 'TB'] as const

/**
 * A byte count as a person reads it. Whole bytes stay whole — a count is a count — and
 * anything scaled carries at most one decimal, which is the precision at which two of
 * these are worth comparing.
 */
function bytes(value: number): string {
  let scaled = value
  let unit = 0
  while (scaled >= 1000 && unit < BYTE_UNITS.length - 1) {
    scaled /= 1000
    unit += 1
  }
  // Re-check after rounding, not only before it: 999,999 B divides to 999.999 kB, which
  // is under the threshold going in and renders `1,000 kB` coming out. The scale has to
  // be chosen against the number that will be shown, not the one being carried.
  if (unit < BYTE_UNITS.length - 1 && Number(scaled.toFixed(unit === 0 ? 0 : 1)) >= 1000) {
    scaled /= 1000
    unit += 1
  }
  const digits = unit === 0 ? 0 : 1
  return `${scaled.toLocaleString('en-US', { maximumFractionDigits: digits })} ${BYTE_UNITS[unit]}`
}

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
     * The leading hex of the source blob's BLAKE3, rendered beside the download link.
     * `DATA.md` §5.1 requires the hash be on screen so a user can check what they got
     * against what the card claimed. Twelve characters is enough to compare against the
     * head of `b3sum`'s output by eye, and the whole digest is on the element's title
     * for anyone who wants to check all of it.
     */
    shortHash: (hash: string) => hash.slice(0, 12),
    /**
     * What the file costs on disk, and what it cost before compression. Both figures,
     * because the pair is the point: one number alone cannot say whether zstd bought
     * anything on this part.
     */
    storedCompressed: (stored: number, ingested: number) =>
      `${bytes(stored)} on disk, compressed from ${bytes(ingested)}`,
    /**
     * The other half. A 3MF is a deflate zip already, so the ingest policy stores it
     * as-is and the two figures would agree — this says so in words rather than showing
     * the same number twice. In words, and not in a tooltip: whether a part is
     * compressed is something the card states, not something a user has to hover to
     * find.
     */
    storedRaw: (stored: number) => `${bytes(stored)} on disk, stored uncompressed`,
    /**
     * Neither claim. A compressed part whose ingested size did not arrive cannot be
     * described by either sentence above: `storedCompressed` needs the second figure,
     * and `storedRaw` would state the opposite of what the row says. Falling back to
     * `storedRaw` there is not the weaker claim, it is a false one — CLAUDE.md's
     * measurement rule forbids the card asserting a compression state it does not have
     * the numbers for.
     */
    storedSize: (stored: number) => `${bytes(stored)} on disk`,
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
  upload: {
    /**
     * The drop target's own label. Names the two ways in, because they are genuinely
     * different gestures and a target that mentions only one reads as though the other
     * will not work — dragging a *folder* is the case most people will try first and the
     * one a plain file input cannot do.
     */
    dropHere: 'Drop a folder here, or',
    choose: 'choose a folder',
    /** Shown while the drag is over the target, so the page says the drop will land. */
    dropNow: 'Release to add these files',
    /**
     * Hashing runs before anything is sent, and on a large drop it is the longest silent
     * stretch in the whole flow, so it says what it is doing rather than only that it is
     * busy. `DATA.md` §5.2: the hash is what lets the next step skip files this library
     * already holds.
     */
    hashing: (done: number, total: number) =>
      `Reading ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} files — checking which ones are already here.`,
    probing: 'Asking which files are new…',
    /**
     * Bytes, not a percentage: a percentage of a 25 GB drop shows the same number for
     * minutes at a time, and the two byte figures answer "how much is left" directly.
     */
    transferring: (sent: number, total: number, files: number, filesTotal: number) =>
      `Uploading ${bytes(sent)} of ${bytes(total)} — file ${Math.min(files + 1, filesTotal).toLocaleString('en-US')} of ${filesTotal.toLocaleString('en-US')}.`,
    committing: 'Finishing the upload…',
    /**
     * What the probe saved, said once, after the transfer is queued. Both halves are
     * worth reporting and they are not the same thing: `alreadyHere` is a file this
     * library already indexes at that path, and `skipped` is bytes some library already
     * holds, so only the rows had to be written. The second is the larger number on a
     * re-import into a new library and is invisible without this line.
     */
    saved: (alreadyHere: number, bytesSkipped: number) => {
      const parts: string[] = []
      if (alreadyHere > 0) {
        parts.push(`${alreadyHere.toLocaleString('en-US')} already here`)
      }
      if (bytesSkipped > 0) {
        parts.push(`${bytes(bytesSkipped)} already stored`)
      }
      return parts.length === 0 ? '' : `Skipped ${parts.join(' and ')}.`
    },
    /** Nothing in the drop was worth sending, which is a success and reads as one. */
    nothingToDo: 'Every file in that folder is already in this library.',
    /**
     * The transfer failed part-way. Deliberately says the upload can be repeated rather
     * than that it can be *resumed*: the client resumes automatically from whatever the
     * server holds, so what the user has to do is the same gesture again, and explaining
     * the offset machinery would be explaining our implementation to someone who wants
     * their files in.
     */
    failed:
      'The upload did not finish. Drop the same folder again — files that already arrived will not be sent twice.',
    /**
     * A drop the browser reported no files for. Almost always an empty folder or a drag
     * that ended outside the target, and neither is an error worth a red banner.
     */
    empty: 'That drop contained no files.',
  },
  detail: {
    loading: 'Loading this part…',
    /**
     * Both a 404 and a network failure land here. They are one message because the
     * remedy is one action: this page was reached from a grid that may be stale, and
     * going back is what refreshes it.
     */
    failed:
      'Could not open this part. It may have been deleted, or the api service may be unreachable — go back to the grid and try again.',
    back: 'Back to the grid',
    /**
     * The badge every mesh-derived figure carries. `CLAUDE.md` makes it non-negotiable,
     * and unlike the card's version this one is per figure: a part can carry an analytic
     * volume beside a tessellated surface area from Phase 2 on, and one badge over the
     * whole page would be wrong about one of them whichever way it is set.
     */
    approximate: '≈',
    approximateTitle:
      'Derived from tessellated geometry rather than read from an analytic CAD entity.',
    exactTitle: 'Read from an analytic CAD entity.',
    geometry: 'Geometry',
    file: 'File',
    size: 'Size',
    identity: 'Identity',
    triangles: 'Triangles',
    /** The count alone: this row is already labelled, unlike the card's line, which
     *  has to carry the word. Locale formatting belongs here rather than in the
     *  component — a bare 'en-US' in a component is a string the translator never
     *  sees, which is the whole rule. */
    trianglesValue: (count: number) => count.toLocaleString('en-US'),
    boundingBox: 'Bounding box',
    /** Three axes in millimetres. Not a volume — a box a part has to fit inside. */
    boundingBoxValue: (mm: readonly number[]) =>
      mm.map((axis) => axis.toLocaleString('en-US', { maximumFractionDigits: 1 })).join(' × ') +
      ' mm',
    volume: 'Volume',
    /**
     * Shown instead of a number when the mesh is open. "Measurement must not lie"
     * includes declining to measure: signed-volume integration over a non-watertight
     * mesh produces a plausible number that means nothing, so ingest writes none and
     * this says why rather than rendering a blank a user would read as zero.
     */
    volumeUnavailable: 'Not available — the mesh is not closed',
    volumeValue: (mm3: number) =>
      `${(mm3 / 1000).toLocaleString('en-US', { maximumFractionDigits: 2 })} cm³`,
    surfaceArea: 'Surface area',
    surfaceAreaValue: (mm2: number) =>
      `${(mm2 / 100).toLocaleString('en-US', { maximumFractionDigits: 2 })} cm²`,
    watertight: 'Watertight',
    watertightYes: 'Closed',
    watertightNo: 'Open',
    format: 'Format',
    /** The path the part is known by, which since slice 6a is its identity in the
     *  library — two parts called `bracket` in two folders are told apart by this. */
    sourcePath: 'Path',
    revision: 'Revision',
    kernel: 'Measured by',
    sourceHash: 'Source hash',
    /**
     * The L0 tessellation. Named for what it is to a user — the thing a viewer will
     * paint — rather than "tessellation_l0", which is a column name.
     */
    preview3d: '3D preview data',
    download3d: 'Download',
    /** With the size, when it is known — a rung is worth knowing the weight of
     *  before clicking, and it is the one figure that says how detailed it is. */
    download3dSized: (stored: number) => `Download · ${bytes(stored)}`,
    /** No rung: a part ingested before the LOD ladder, or one still in the queue. */
    noPreview3d: 'Not generated yet',
    /** A figure the database has no value for. Distinct from a refusal to measure. */
    unknown: 'Unknown',
  },
  scan: {
    /**
     * Before the walk finishes there is no file count to report — the worker is still
     * reading the directory. Saying "0 of 1 files" there is not a smaller claim than the
     * truth, it is a wrong one: the 1 is the walk itself.
     */
    walking: 'Reading the folder…',
    /**
     * Shown while a batch is draining. `done` counts every file the worker has finished
     * with, however it finished — ingested, skipped and failed alike — because what this
     * line answers is "how much is left", and a file that failed is not still pending.
     *
     * Both numbers are **files**, not jobs. The walk is a job in the same batch, so the
     * caller subtracts `BatchStatus.scanned` from both halves before calling this — a
     * three-file folder read "Scanning — 1 of 4 files" until a review measured it.
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
    /** The action bar's trigger. Names the mount rather than the button's effect,
     * because what a scan reads is a folder on the server and not anything on this
     * page — the empty state below says the same thing at more length.
     */
    start: 'Scan the ingest folder',
    /**
     * The scan could not be queued at all. Distinct from a scan that ran and failed:
     * nothing was written, so trying again is the whole remedy.
     */
    startFailed:
      'Could not start the scan. Check that the api service is running, then try again.',
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
    /**
     * The `GET` did not answer, so the toggle has no position to take. It stays in the
     * mixed state rather than falling back to the documented default: the default is what
     * a library is set to until someone changes it, not what this one is, and a control
     * that shows a confident "on" for a library that is off is the mistake this read
     * exists to close.
     */
    autoThumbnailUnknown:
      'Could not read whether previews are generated automatically here. Check that the api service is running, then reload.',
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
  /**
   * Why a job failed, as the handler wrote it. `scan.failed` and `render.failed` above
   * are counts, and a count cannot tell an operator that `/ingest` is not mounted — the
   * reason can, and `BatchStatus.failed` has carried it since slice 2 with nothing
   * displaying it. Neutral between a scan and a render because the message itself says
   * which it was.
   */
  failure: {
    /**
     * A `derive` failure falls through to the part its revision belongs to, and a
     * `scan_directory` failure has no path at all — it is the directory that failed, and
     * the reason names it. So an empty path renders as the reason alone rather than as a
     * dangling separator.
     */
    line: (path: string, reason: string) => (path === '' ? reason : `${path} — ${reason}`),
    /**
     * `BatchStatus.failed` is capped at 100 while `failedTotal` is the real number. A
     * list that silently stops at 100 is a measurement that lies by omission, which is
     * the same fault the truncated grid needs `parts.showingFirstPage` for.
     */
    more: (hidden: number) =>
      hidden === 1
        ? 'And 1 more not listed here.'
        : `And ${hidden.toLocaleString('en-US')} more not listed here.`,
  },
  /**
   * The per-card download control. Its own group rather than a field on `parts`: this is
   * the one thing on this page that hands a user their own bytes back, and `DATA.md`
   * §5.1 is the section it answers to.
   */
  download: {
    original: 'Download',
    /**
     * The accessible name, because the visible label is identical on every card and
     * "Download" alone does not say what of. Same shape as `render.partFor`.
     */
    originalFor: (name: string) => `Download the original file for ${name}`,
    /**
     * The revision has no source `file` row, so there are no bytes to hand over and no
     * link to render. Not a disabled control: pressing it again would not help, and
     * something that cannot work must not look like something that can. Says what does
     * help instead, because a re-scan is what re-attaches a source to a part in this
     * state.
     */
    noSource:
      'No source file on this revision, so there is nothing to download. Re-scan the library to attach one.',
  },
  /**
   * What the library occupies, split by the storage classes `DATA.md` §1.1 splits it
   * into. Sources are the bytes nothing can regenerate; derivatives and previews are the
   * bytes something can, which is why the ratio between them is the figure worth showing
   * and neither total is worth showing alone.
   */
  storage: {
    /**
     * Both totals are bytes on disk after compression, deduplicated — bytes two parts
     * share are counted once, because that is what the volume holds.
     */
    totals: (source: number, derivative: number, ratio: number | null) =>
      // `typeof`, not `=== null`: the response is cast rather than validated, so a field
      // the server stops sending arrives as undefined, which `=== null` waves through
      // into `(undefined * 100)` and renders `NaN% of source`. Same defence `SourceFile`
      // applies to its three fields, applied to the one field this component reads.
      typeof ratio !== 'number'
        ? `Sources ${bytes(source)} on disk · derivatives ${bytes(derivative)}.`
        : `Sources ${bytes(source)} on disk · derivatives ${bytes(derivative)}, ${(ratio * 100).toLocaleString('en-US', { maximumFractionDigits: 1 })}% of source.`,
    failed:
      'Could not read what this library occupies. Check that the api service is running, then reload.',
  },
  emptyLibrary: {
    title: 'Nothing here yet',
    body:
      'This library is empty. Drop a folder of models above to add them, or scan the directory mounted on the server — either way, every model found appears here.',
  },
} as const
