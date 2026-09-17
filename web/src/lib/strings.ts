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

/**
 * The two things a category holds, counted and named. Separate builders rather than one
 * sentence per combination: the delete confirmation has to state both, and nine hand-
 * written variants is nine places for one of them to go missing — which is exactly how the
 * subcategories dropped out of that copy in the first place.
 */
function countedModels(count: number): string {
  if (count === 0) return 'no models'
  return count === 1 ? '1 model' : `${count.toLocaleString('en-US')} models`
}

function countedSubcategories(count: number): string {
  if (count === 0) return 'no subcategories'
  return count === 1 ? '1 subcategory' : `${count.toLocaleString('en-US')} subcategories`
}

/**
 * What a finished batch did with its files: added, then revised, then already here, each
 * only when it happened. One builder for the scan's line and the upload's, which differ only
 * in the words before the dash.
 */
function settled(ingested: number, revised: number, skipped: number): string {
  const clauses = [`${ingested.toLocaleString('en-US')} added`]
  if (revised > 0) clauses.push(`${revised.toLocaleString('en-US')} revised`)
  if (skipped > 0) clauses.push(`${skipped.toLocaleString('en-US')} already here`)
  return clauses.join(', ')
}

/**
 * Files whose bytes changed in a hobby library (Phase 4 slice 1). A notice, not a failure —
 * nothing broke, and the new bytes are still where they came from — so it says what happened
 * and the two ways to keep such a change, and nothing about trying again.
 */
function notKept(unkept: number): string {
  if (unkept === 0) return ''
  const files =
    unkept === 1
      ? '1 file changed but was'
      : `${unkept.toLocaleString('en-US')} files changed but were`
  return ` ${files} not kept: this library keeps no revisions. Switch it to keep every change, or give a file a new name to add it as a new part.`
}

/**
 * A figure's change between two revisions: signed, and with its share of where it started
 * when that was not zero. No change says so in words, because "+0 cm³" reads as a
 * measurement of something. A change too small for two places says it is under 0.01, because
 * "−0 mm" reads as no change with a sign, and "No change" would hide it.
 */
function change(value: number, unit: string, percent: number | null): string {
  if (value === 0) return 'No change'
  const sign = value > 0 ? '+' : '−'
  const amount = Math.abs(value).toLocaleString('en-US', { maximumFractionDigits: 2 })
  const share =
    percent === null
      ? ''
      : ` (${sign}${Math.abs(percent).toLocaleString('en-US', { maximumFractionDigits: 1 })}%)`
  const shown = amount === '0' ? 'under 0.01' : `${sign}${amount}`
  return `${shown}${unit === '' ? '' : ` ${unit}`}${share}`
}

/**
 * The counted phrases above read mid-sentence as well as at the head of one — "no models"
 * has to stay lowercase where a sentence has already started — so the one place that opens
 * a sentence with one raises its first letter itself.
 */
function opensSentence(clause: string): string {
  return clause.charAt(0).toUpperCase() + clause.slice(1)
}

/** A measured length or angle, to the thousandth the Phase 3 exit reads it at: `22.000`, never `22`. */
function fixed(value: number): string {
  return value.toLocaleString('en-US', { minimumFractionDigits: 3, maximumFractionDigits: 3 })
}

export const strings = {
  appName: 'Lapidary',
  /**
   * What the browser tab says. WCAG 2.2 SC 2.4.2 is Level A and asks that a page be
   * titled by topic or purpose; `index.html` carries one static title for the whole
   * application, which is a title for none of these pages.
   *
   * Product name last, subject first: a person with six part pages open reads the tab
   * strip left to right and the first twelve characters are all they get.
   */
  titles: {
    library: 'Parts — Lapidary',
    /**
     * The part's own name when the page has it, and a plain heading while it loads or
     * when it does not exist. Named rather than numbered — a page called `LP-1042-03`
     * is findable in a tab strip and one called `Part 0193…` is not.
     */
    part: (name: string | null) => (name === null ? 'Part — Lapidary' : `${name} — Lapidary`),
    removed: 'Removed parts — Lapidary',
    sharing: 'Shared libraries — Lapidary',
  },
  /**
   * The bar above the grid: where you are, what you are looking for, how it is laid out, and
   * what can be done to the library. `v2` draws all of it in one row; the menus are what let
   * it fit, and every control inside them is the same control it was when it had a row of
   * its own.
   */
  /** The header every page shares. */
  frame: {
    /** The accessible name of the header's nav — "navigation" alone says nothing. */
    places: 'Places',
    /** The grid of a library's parts, as a place beside Removed and Sharing. */
    parts: 'Parts',
    /** The narrow-screen button that slides the places, and a grid's categories and filters, in over the page. */
    openDrawer: 'Menu',
    closeDrawer: 'Close menu',
  },
  toolbar: {
    view: 'View',
    library: 'Library',
    layout: 'Layout',
    upload: 'Upload a folder',
    /** A bundle exported from Lapidary, its parts and their history (Phase 4 slice 2). */
    importBundle: 'Import a bundle',
    importFailed: 'Could not import the bundle. Check that the api service is running, then try again.',
  },
  /**
   * Bulk selection on the grid. Off until the toolbar's Select is pressed, so a card keeps its
   * one tab stop the rest of the time.
   */
  selection: {
    toggle: 'Select',
    bar: 'Selected parts',
    /** Each checkbox names its part: forty checkboxes all called "Select" say nothing. */
    selectPart: (name: string) => `Select ${name}`,
    count: (count: number) =>
      count === 1 ? '1 part selected' : `${count.toLocaleString('en-US')} parts selected`,
    clear: 'Clear selection',
    /**
     * "selected parts" rather than a bare "parts": `no-bare-strings.test.ts` treats every fixed
     * piece of a template here as prose, and a lone "parts" would match the grid's own
     * `['parts', library]` cache keys.
     */
    moveTitle: (count: number) =>
      count === 1
        ? 'Move the selected part'
        : `Move the ${count.toLocaleString('en-US')} selected parts`,
    working: (done: number, total: number) =>
      `${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} done…`,
    /**
     * After a bulk action, the parts it did not change and why, one line each. The rest were
     * changed; the ones listed stay selected, so trying again is one press.
     */
    failedHeading: (count: number) =>
      count === 1 ? '1 part was not changed:' : `${count.toLocaleString('en-US')} parts were not changed:`,
    failure: (name: string, reason: string) => `${name} — ${reason}`,
    /** A bundle of the selection (Phase 4 slice 2): every revision's original file and a manifest. */
    exportBundle: 'Export bundle',
    exporting: (count: number, revisions: number, size: number) =>
      `Exporting ${count === 1 ? 'the selected part' : `the ${count.toLocaleString('en-US')} selected parts`} with ${revisions.toLocaleString('en-US')} ${revisions === 1 ? 'revision' : 'revisions'} (${bytes(size)}). Your browser saves the bundle.`,
    exportFailed: 'Could not export the selection. Check that the api service is running, then try again.',
  },
  layouts: {
    detail: 'Detail',
    gallery: 'Gallery',
    list: 'List',
  },
  /**
   * The first tab stop on the library page, visible only once focused.
   *
   * SC 2.4.1, Level A. The category tree is dozens of tab stops that repeat on every
   * visit, and it sits before the grid in the source order — without this, reaching the
   * first part by keyboard means tabbing through every category first.
   */
  skipToParts: 'Skip to the parts',
  /**
   * The last thing the application can say. A crash inside a route unmounts everything
   * below it, so this replaces the page rather than annotating it — and it names the
   * one action that has ever fixed one, rather than apologising.
   */
  crash: {
    title: 'This page stopped working',
    body: 'Something in the application failed rather than something in your library — your parts and files are untouched. Reloading usually clears it.',
    reload: 'Reload the page',
  },
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
    approximate: 'approximate',
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
     * Some of the library, with more to come. Deliberately not "showing 50 of 1,000":
     * the server does not count the library to answer a page, and adding a count query to
     * every grid request to render one number would be paying for it on every scroll.
     * What the user needs to know is that there is more, and how to get it.
     */
    showingSoFar: (count: number) => `${count.toLocaleString('en-US')} parts so far.`,
    loadMore: 'Load more',
    loadingMore: 'Loading…',
  },
  upload: {
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
     * An upload's batch, once its files are committed. Its own copy and not the scan's: an
     * upload has no walk job, so `scan.walking` said "Reading the folder…" for as long as the
     * files were adding, and `scan.finished` called files nobody scanned a scan.
     */
    batchRunning: (done: number, total: number) =>
      `Adding — ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} files.`,
    batchFinished: (ingested: number, skipped: number, revised = 0, unkept = 0) =>
      `Upload complete — ${settled(ingested, revised, skipped)}.${notKept(unkept)}`,
    /**
     * The upload batch this page started cannot be read back. Never a mistyped id — it came
     * from the commit's `202` — so the advice is to reload, as `render.unknown`'s is.
     */
    batchUnknown:
      'Could not read how the upload is going. The files are queued and continue on the server; reload to pick it up again.',
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
    /**
     * What a screen reader hears at each ≈, which is hidden from it. `CLAUDE.md` asks for the
     * word; the symbol is its compact form for the eye. The leading space keeps it from
     * running into the figure it follows.
     */
    approximateSpoken: ' approximate',
    /**
     * One line under the Geometry heading whenever a figure there is mesh-derived, so the ≈
     * each carries is explained in words rather than left as a symbol to learn. A key and
     * not a badge over the section: the marks stay per figure, for the reason `approximate`
     * gives.
     */
    approximateKey: 'Approximate — measured from the mesh, not read from CAD geometry.',
    geometry: 'Geometry',
    file: 'File',
    size: 'Size',
    identity: 'Identity',
    triangles: 'Triangles',
    /** A CAD revision's B-rep faces and edges, in the comparison between two revisions. */
    faces: 'Faces',
    edges: 'Edges',
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
    assembly: 'Assembly',
    /** Leaves counted with every instance, beside the distinct part definitions they place. */
    assemblyCounts: (parts: number, prototypes: number) =>
      `${parts.toLocaleString('en-US')} ${parts === 1 ? 'part' : 'parts'}, ${prototypes.toLocaleString('en-US')} distinct`,
    assemblyFailed: 'Could not load the assembly tree. Reload the page to try again.',
    /** A part, or a branch of parts, taken out of the 3D view and put back. */
    hide: 'Hide',
    show: 'Show',
    isolate: 'Isolate',
    showAll: 'Show all parts',
    /** The buttons' whole names, so a screen reader hears which part each acts on. */
    hidePart: (name: string) => `Hide ${name}`,
    showPart: (name: string) => `Show ${name}`,
    isolatePart: (name: string) => `Show only ${name}`,
    /** A part's revisions, shown once it has more than one (Phase 4 slice 1). */
    history: 'History',
    historyRevision: (label: string) => `Revision ${label}`,
    /** Where a revision's bytes came from, in the words a person would use for it. */
    origin: { ingest: 'Scanned', upload: 'Uploaded', agent: 'Saved from a checkout' },
    historyDate: (iso: string) =>
      new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' }),
    historyFailed: "Could not load this part's history. Reload the page to try again.",
    /** Changes between revisions, in each figure's own unit. */
    volumeChange: (mm3: number, percent: number | null) => change(mm3 / 1000, 'cm³', percent),
    areaChange: (mm2: number, percent: number | null) => change(mm2 / 100, 'cm²', percent),
    lengthChange: (mm: number, percent: number | null) => change(mm, 'mm', percent),
    countChange: (count: number, percent: number | null) => change(count, '', percent),
    massChange: (grams: number, percent: number | null) => change(grams, 'g', percent),
    compare: 'Compare',
    compareFrom: 'From',
    compareTo: 'To',
    compareFigure: 'Figure',
    compareChange: 'Change',
    /** Volume times a typed density: never a measurement, so always shown with ≈. */
    mass: 'Mass',
    /** Under a comparison that has a mass row. */
    massNote:
      'Mass is each revision’s volume times the density of the part’s material as it is now, so both revisions use today’s material and density.',
    boundingBoxAxis: (axis: 0 | 1 | 2) => `Bounding box ${'XYZ'[axis]}`,
    /** The centre of a revision's volume, per axis: exact from a B-rep, ≈ from a mesh. */
    centreAxis: (axis: 0 | 1 | 2) => `Centre of mass ${'XYZ'[axis]}`,
    /** A figure one of the two revisions did not record: no change can be read off it. */
    notInBoth: 'Not measured in both',
    compareFailed: 'Could not compare these revisions. Reload the page to try again.',
    /** The overlay: the From revision drawn as a grey ghost over the part (Phase 4 slice 2). */
    ghost: 'Show From as a ghost in the 3D view',
    ghostNoMesh: (label: string) => `Revision ${label} has no mesh to draw as a ghost.`,
    ghostCoarse:
      'The ghost is that revision’s coarse preview, so a small difference along its outline may be the preview rather than the part.',
    /** Who has the part checked out, and since when (Phase 4 slice 1). */
    checkedOut: 'Checked out',
    /** A part pulled from somebody's shared library (sharing S3): who it came from. */
    sharedBy: 'From',
    checkedOutBy: (holder: string, since: string) =>
      `${holder}, since ${new Date(since).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' })}`,
    releaseLock: 'Release…',
    releaseLockTitle: 'Release this check-out?',
    releaseLockBody: (holder: string) =>
      `${holder} will not be able to save their changes back: their next save is refused, and the refusal says the lock was released here. Release it only if they cannot check it in themselves.`,
    releaseLockConfirm: 'Release',
    releaseLockFailed: 'The check-out was not released. Reload the page, then try again.',
    /** Recorded as who released a lock. The page has no signed-in person to name yet. */
    releasedBy: 'the part page',
  },
  /**
   * The three-step removal, and the wording rules `CLAUDE.md` makes non-negotiable:
   * *"We never delete user data implicitly. Delete is soft. Purge is separate and
   * explicit. Blobs quarantine 30 days before removal. Derivative cache eviction is a
   * different action with different wording and must never read as data loss."*
   *
   * Three actions with three vocabularies, and the differences are load-bearing:
   *
   * - **Remove** never says "delete". It is reversible indefinitely and touches no bytes,
   *   so language that implies destruction would make people hesitate over an action that
   *   costs nothing — and would spend the alarm that purge actually needs.
   * - **Purge** says "permanently", names the part, and is never reachable in one click
   *   from the library. It is the only word here that means what it says.
   * - **Eviction** (Phase 4, with the tiering job) says "free cache space" and never
   *   "delete": it removes only bytes we produced and can reproduce, which is why it is
   *   the one of the three that needs no undo. Its strings live here when it ships;
   *   the boundary is written down now so the second one does not borrow the first one's
   *   words.
   *
   * No number here is ever labelled "freed". A purge frees nothing on the day it runs.
   */
  removal: {
    remove: 'Remove from library',
    /** Said before the click, so the reassurance arrives when the hesitation does. */
    removeHint: 'Hidden from the library. Nothing on disk changes, and you can restore it.',
    removing: 'Removing…',
    removeFailed: 'Could not remove this part. Nothing changed — try again.',
    restore: 'Restore',
    restoring: 'Restoring…',
    restoreFailed: 'Could not restore this part. It is still removed — try again.',
    /** The list is the only route back to a removed part: every other read path filters
     *  them out, so without this delete would be a one-way door. */
    removedTitle: 'Removed parts',
    removedEmpty: 'Nothing has been removed from this library.',
    removedLead:
      'These are hidden from the library and still on disk. Restore one at any time, or purge it to start the 30-day countdown before its bytes are removed.',
    removedCount: (count: number) =>
      count === 1 ? '1 removed part' : `${count.toLocaleString('en-US')} removed parts`,
    purge: 'Purge permanently',
    /**
     * Names the part by its *path*, not its name, because a confirmation that cannot be
     * answered correctly is worse than none: since slice 6a the path is what tells two
     * parts called `bracket` apart, and this is the one irreversible action in the
     * product. The second sentence is the one fact that makes this survivable, and it is
     * stated as a deadline rather than a promise of recovery — there is no restore button
     * after this, only a hash and an operator.
     */
    purgeConfirm: (sourcePath: string) =>
      `Purge “${sourcePath}” permanently? Its part, revision and file records are removed now. Bytes nothing else uses are kept for 30 days, then deleted.`,
    purging: 'Purging…',
    purgeFailed: 'Could not purge this part. Nothing was removed — try again.',
    /** Deliberately not "freed". Nothing is freed today; these bytes are waiting. */
    purgedNothing: 'Purged. Its bytes are still in use by another part and were kept.',
    purgedQuarantined: (blobs: number, stored: number) =>
      blobs === 1
        ? `Purged. 1 file (${bytes(stored)}) is kept for 30 days, then deleted.`
        : `Purged. ${blobs.toLocaleString('en-US')} files (${bytes(stored)}) are kept for 30 days, then deleted.`,
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
    finished: (ingested: number, skipped: number, revised = 0, unkept = 0) =>
      `Scan complete — ${settled(ingested, revised, skipped)}.${notKept(unkept)}`,
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
   * Moving a library's source files out of the shared content-addressed store and into
   * each part's own directory. The worker queues this itself on startup for a library
   * upgraded from before folders existed — nothing on this page starts one — so this
   * copy exists to make a batch already running, watched by its id in the URL, read as
   * what it is rather than as a scan of the mounted directory.
   *
   * Its own copy for the same reason `render`'s is: a migration ingests nothing and
   * skips nothing, so `scan.finished` would report "Scan complete — 0 added." over a
   * corpus that just moved, and an operator watching their own files get relocated
   * deserves a line that says so.
   *
   * Neither line carries a number, and that is deliberate rather than a gap to fill in
   * later. `BatchStatus.migrated` counts settled `migrate_storage` JOBS, and one job
   * moves up to `HASHES_PER_RUN` (200) files — so a 1,000-file library finishing in five
   * runs would read "5 files moved" if this counted jobs as files, which is the exact
   * mistake `BatchStatus.scanned`'s own doc says CLAUDE.md's measurement rule forbids.
   * No column on a `job` row counts files actually moved, so there is nothing honest to
   * put a number to yet — the fix is copy that does not claim a count it does not have,
   * not a number that quietly means something else.
   *
   * `failed` and `unknown` exist for the reason `render`'s do: without them the progress
   * line falls back to `scan`'s, and `scan.failed` says the files "will not appear in the
   * grid" — about models that already exist, are already in the grid, and whose bytes were
   * never at risk. A migration moves a file a part already has; the worst it can do is
   * leave that file where it was.
   */
  migrate: {
    running: 'Moving files into their model folders…',
    /**
     * `failedTotal` is 0 for the whole batch or it is not, and the line below reports
     * which — the number itself belongs to `failed`, and the sentence here must not go on
     * claiming every file arrived while that line says some did not.
     */
    finished: (failed: number) =>
      failed === 0
        ? "Move complete — this library's files are now in their model folders."
        : 'Move finished, but not every file could be moved. The ones that could are in their model folders; the rest are still in the shared store, and every model is still listed.',
    /**
     * Uncounted, alone among the three `failed` strings, and deliberately so. The number
     * the progress line has is `failedTotal`, which counts `migrate_storage` JOBS — each a
     * run over a page of up to 200 hashes — so printing it as files would report 200 files
     * as 1, the measurement mistake the doc above is entirely about. Naming a unit of its
     * own instead ("2 steps") only moves the problem: nothing else on screen says what a
     * step is, while the failure lines directly below this one name the parts and say what
     * happened to each. Those lines are the count, and they are already rendered.
     *
     * What the sentence is for is the part no list conveys: a migration failure is not a
     * loss. Nothing was deleted, nothing left the grid, and the models involved still open
     * from where they always did.
     */
    failed: () =>
      'Not every file could be moved — the failures are listed below. Those files stay where they already were: nothing was removed, and every model is still listed.',
    /**
     * The migration's status cannot be read back. Reachable only after a poll has already
     * identified this batch as a migration, so the work is genuinely on the server and
     * genuinely continues — the advice is to reload, not to check what was typed.
     */
    unknown:
      'Could not read how the file move is going. It was queued by the server and continues there; reload to pick it up again.',
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
     * list that silently stops at 100 is a measurement that lies by omission — the same
     * fault `parts.showingSoFar` exists to avoid on the grid, which says there is more
     * rather than letting a capped page read as the whole library.
     */
    more: (hidden: number) =>
      hidden === 1 ? 'Show 1 more' : `Show ${hidden.toLocaleString('en-US')} more`,
    moreFailed:
      'The rest of the list did not load. Check the connection, then press the button again.',
    retry: 'Retry',
    /**
     * The per-row button's accessible name. A list of buttons all named "Retry" tells a
     * screen reader nothing about which file each one retries.
     */
    retryOne: (path: string) => (path === '' ? 'Retry this job' : `Retry ${path}`),
    /** Offered only with two failures or more; with one, the row's own Retry is that button. */
    retryAll: (count: number) => `Retry all ${count.toLocaleString('en-US')}`,
    retryFailed:
      'The retry did not reach the server. Check the connection, then press Retry again.',
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
    /** A controlled library's part, opened in a desktop app through `lapidary://` (Phase 4 slice 2). */
    openInApp: 'Open in desktop app',
    /** DATA §6.3's honesty line: which tools send a save back, and the way without the agent. */
    openInAppNote:
      'Needs lapidary register and lapidary agent on this computer. Rhino, FreeCAD and Blender save back as a new revision; Fusion 360 and Onshape keep their own copy, so export from them and upload instead. Without the agent, use Download.',
    /**
     * A part no slicer reads as it is, a STEP or IGES part, handed to one as a 3MF Lapidary writes
     * from its mesh. Beside Download, which stays the part's own file.
     */
    forSlicer: '3MF for a slicer',
    forSlicerBuilding: 'Writing the 3MF…',
    forSlicerReady: 'Download the 3MF',
    /** The worker's or the server's own message, which says what broke and what to do. */
    forSlicerFailed: (reason: string) => `The 3MF was not written: ${reason}`,
  },
  /**
   * What the library occupies, split by the storage classes `DATA.md` §1.1 splits it
   * into. Sources are the bytes nothing can regenerate; derivatives and previews are the
   * bytes something can, which is why the ratio between them is the figure worth showing
   * and neither total is worth showing alone.
   */
  storage: {
    /** The rail's disclosure: what the library and the store occupy, and the server's state. */
    title: 'Storage',
    size: (value: number) => bytes(value),
    /**
     * Both totals are bytes on disk, and the two halves are counted differently because
     * the store holds them differently: one source file per part, counted per part, since
     * a browsable store keeps a copy inside each model's own folder; derivatives
     * deduplicated, because those are still content-addressed and genuinely shared.
     * `PgParts::storage_totals` is where that accounting is written down, including what
     * it leaves out.
     */
    totals: (source: number, derivative: number, ratio: number | null) =>
      // `typeof`, not `=== null`: the response is cast rather than validated, so a field
      // the server stops sending arrives as undefined, which `=== null` waves through
      // into `(undefined * 100)` and renders `NaN% of source`. Same defence `SourceFile`
      // applies to its three fields, applied to the one field this component reads.
      typeof ratio !== 'number'
        ? `Sources ${bytes(source)} on disk · derivatives ${bytes(derivative)}.`
        : `Sources ${bytes(source)} on disk · derivatives ${bytes(derivative)}, ${(ratio * 100).toLocaleString('en-US', { maximumFractionDigits: 1 })}% of source.`,
    /**
     * Appended when the library holds removed parts, and the wording is the point: they
     * are *still on disk*, not freed. Without this clause the totals above fall the moment
     * somebody removes a part and the volume does not, which reads as a saving — the one
     * reading `CLAUDE.md` says this area must never produce.
     *
     * Says nothing about quarantined bytes, which have no library to belong to: a purge
     * removes the part chain, so those blobs are counted by no library's panel. That gap
     * is real, was recorded in the slice 7 design as closing with Phase 4 — and is closed
     * instead by `everything` below, which is the panel that *can* report them.
     */
    removed: (stored: number) => ` ${bytes(stored)} removed, still on disk.`,
    failed:
      'Could not read what this library occupies. Check that the api service is running, then reload.',

    /**
     * The whole store, across every library, and the two figures no per-library line can
     * carry: `removed` is on the disk until somebody purges it, and `quarantined` is on the
     * disk *and* belongs to no library at all, because the part that would have said which
     * one is the part that was purged.
     *
     * Both clauses are conditional for the reason `removed` above is: a permanent
     * "0 B quarantined" is noise on every installation where nobody has purged anything,
     * and the moment one exists it is bytes nothing else in the application admits to.
     */
    everything: (
      source: number,
      derivative: number,
      inline: number,
      removed: number,
      quarantined: number,
    ) =>
      `Everything: ${bytes(source + derivative + inline + removed + quarantined)} across all libraries — ${bytes(source)} of models, ${bytes(derivative)} of generated views, ${bytes(inline)} of thumbnails in the database${removed > 0 ? `, ${bytes(removed)} removed and still on disk` : ''}${quarantined > 0 ? `, ${bytes(quarantined)} purged and waiting out its 30 days` : ''}.`,
    /**
     * The walk is opt-in because it costs the server a look at every file. Worded as the
     * question it answers rather than as the work it does — "measure" is what the user
     * wants; that it is a directory walk is our problem.
     */
    measureOnDisk: 'Measure what is actually on disk',
    measuring: 'Measuring…',
    /**
     * The render cache (Phase 4 slice 2): previews Lapidary made and can make again. Worded by the
     * eviction boundary above: "free cache space", never "delete", and no number called freed,
     * because the bytes leave only when their quarantine ends.
     */
    renderCache: (size: number) =>
      `${bytes(size)} of detailed 3D previews and slicer files nobody has opened in 90 days. Lapidary makes each one again when it is next needed.`,
    freeCache: 'Free cache space…',
    freeCacheTitle: 'Free cache space?',
    freeCacheBody: (size: number) =>
      `This removes the detailed 3D previews and slicer files of parts nobody has opened in 90 days. No model file is touched: each part still opens at once on its coarse preview while the detailed one is rebuilt, and a slicer file is written again when it is next asked for. ${bytes(size)} goes into the 30-day quarantine and becomes free space when that ends.`,
    freeCacheConfirm: 'Free cache space',
    freeCacheFailed: 'Could not free cache space. Check that the api service is running, then try again.',
    cacheFreed: (removed: number, size: number) =>
      `Removed ${removed} ${removed === 1 ? 'file' : 'files'} from the render cache. ${bytes(size)} becomes free space when its 30-day quarantine ends.`,
    /**
     * The two numbers side by side, and the gap named rather than left to be noticed.
     *
     * The tracked figures count what the database knows about: one row per model file, one
     * per derivative. The disk also holds a `metadata.json` beside every model — deliberately
     * counted by nothing, because a per-manifest length column would be a figure nobody
     * would ever see move — plus anything a person has put in the folder themselves, which
     * is a thing this layout invites. So the disk number is the larger one, and the
     * difference is not an error.
     */
    /**
     * `tracked` here is the part of the figures above that is genuinely *in the storage
     * folder* — so not the thumbnails, which are in Postgres. Measured on a real library
     * before this was written: folding them in put the tracked total 5.7 MB above a walk of
     * the store, which reads as bytes having gone missing rather than as a category error.
     *
     * The disk is then legitimately the larger of the two, by the `metadata.json` beside
     * every model (deliberately counted by nothing) plus whatever the owner has put in the
     * folder — which this layout invites them to do.
     */
    onDisk: (disk: number, tracked: number) =>
      disk >= tracked
        ? `On disk: ${bytes(disk)} in the storage folder. That is ${bytes(disk - tracked)} more than the models and views above, which is the manifest beside each model plus anything you have put in the folder yourself — neither is tracked, both are real. Thumbnails are not in this figure: they live in the database.`
        : `On disk: ${bytes(disk)} in the storage folder, which is ${bytes(tracked - disk)} less than the models and views above. That should not happen — every one of those should be a file. Something has removed files from the store without going through the app.`,
    onDiskFailed:
      'Could not measure the storage folder. The figures above still stand — they come from the database, not from the disk.',
  },
  /**
   * The category tree beside the grid, moving models between categories, and the folder a
   * model is stored in.
   *
   * A category is a location and never an identity (`FolderId`'s own doc says so), and
   * every string here has to keep that true. Moving a model changes where it is filed and
   * where its directory sits on disk. Deleting a category hides what is inside it and
   * leaves every byte alone. Neither is `DATA.md` §1.6's purge, and neither is evicting
   * the derivative cache — three different things this product must never let read as
   * each other, which is why the delete copy below says what stays as plainly as it says
   * what goes.
   */
  /**
   * Saved filters, in the rail above the facets: the grid's filters kept under a name for a library,
   * and shared by everyone who opens it. Removing one takes the name off the list and touches no part.
   */
  /** The marks on small buttons whose words are their `aria-label`s. */
  glyphs: {
    moveUp: '↑',
    moveDown: '↓',
    rename: '✎',
    remove: '×',
  },
  savedFilters: {
    title: 'Saved filters',
    saveThis: 'Save this filter',
    name: 'Name for this filter',
    save: 'Save',
    cancel: 'Cancel',
    remove: (name: string) => `Remove the saved filter ${name}`,
    rename: (name: string) => `Rename the saved filter ${name}`,
    renameLabel: 'New name',
    renameConfirm: 'Rename',
    moveUp: (name: string) => `Move ${name} up`,
    moveDown: (name: string) => `Move ${name} down`,
    /** Beside a saved filter whose category was deleted after it was saved. */
    folderGone: 'category deleted',
    failed: 'Could not load the saved filters. Check that the api service is running, then reload.',
    refusedWithoutReason:
      'The filter was not saved, and the server gave no reason. Try again, and check the server logs if it keeps failing.',
  },
  /** The grid opened on a category deleted since a saved filter or a link named it. */
  categoryGone: {
    title: 'This category was deleted',
    body: 'Its models were removed with it, and Removed parts can bring them back. The rest of these filters still work without it.',
    widen: 'Show these filters without the category',
  },
  /** The grid opened on a field filter the library no longer takes: not offered, removed, or of another kind. */
  fieldGone: {
    title: 'This field no longer filters the grid',
    body: 'Since this filter or link was made, the field was removed, stopped being offered as a filter, or was defined again for a different kind of value. The rest of these filters still work without it.',
    widen: 'Show these filters without the field',
  },
  /** A field filter no grid can take: a link typed by hand, or a bound typed into the range boxes that is not a number. */
  fieldUnreadable: {
    title: 'This field filter is not one the grid can use',
    body: 'It gives the field a value and a range at once, a bound that is not a number, or a range that starts above where it ends. The rest of these filters still work without it.',
    widen: 'Show these filters without the field',
  },
  /** The filters beside the grid. `docs/DATA.md` §3.4. */
  facets: {
    format: 'Format',
    /** A format as ingest records it, the extension, as a person reads it. */
    name: (value: string) => value.toUpperCase(),
    count: (count: number) => count.toLocaleString('en-US'),
    /** A button's whole name, so a screen reader hears the count with the format. */
    option: (value: string, count: number | null) =>
      count === null
        ? value.toUpperCase()
        : `${value.toUpperCase()}, ${count.toLocaleString('en-US')} ${count === 1 ? 'part' : 'parts'}`,
    material: 'Material',
    /** A material's whole name, as the file names it, with its count. */
    materialOption: (value: string, count: number | null) =>
      count === null
        ? value
        : `${value}, ${count.toLocaleString('en-US')} ${count === 1 ? 'part' : 'parts'}`,
    tag: 'Tag',
    /** A tag's whole name, as a person wrote it, with its count. */
    tagOption: (value: string, count: number | null) =>
      count === null
        ? value
        : `${value}, ${count.toLocaleString('en-US')} ${count === 1 ? 'part' : 'parts'}`,
    failed: 'Could not load the formats, materials and tags in this library. Reload to try again.',
  },
  folders: {
    title: 'Categories',
    /**
     * The unfiltered library. Selecting it drops `folderId` from the URL rather than
     * setting it to anything: the parts route reads an absent parameter as "the whole
     * library", and there is no id that means "no category". Selecting a category shows
     * what is in it and in everything under it — the server's filter is
     * subtree-inclusive, because a tree that hides nested models when you click a parent
     * is a tree that lies about what it contains.
     */
    root: 'All models',
    /** The category row's share action, beside Rename and Delete. */
    shareAction: 'Share',
    shareFor: (name: string) => `Share ${name} with the people you are paired with`,
    /** Beside a category this installation shares. */
    shared: 'Shared',
    /** A branch's disclosure, named for its category, since every row's control looks alike. */
    showSubcategories: (name: string) => `Show the categories inside ${name}`,
    hideSubcategories: (name: string) => `Hide the categories inside ${name}`,
    loading: 'Loading categories…',
    failed:
      'Could not load the categories in this library. Check that the api service is running, then reload.',
    empty: 'No categories yet. Scanning or uploading a folder makes them, or add one with New category.',
    moveTo: 'Move to…',
    /** The accessible name, since the visible label is identical on every card. */
    moveToFor: (name: string) => `Move ${name} to another category`,
    moveTitle: (name: string) => `Move ${name}`,
    moveHere: 'Move here',
    /** Same, for the row: "Move here" is the visible label on every row in the chooser. */
    moveInto: (category: string) => `Move here — ${category}`,
    moveFailed:
      'Could not move this model. Check that the api service is running, then try again.',
    /**
     * The move route's `409` carries no `reason` this client recognises — an old server,
     * from before that field shipped, or a value that is not one of the four it documents.
     * The fallback has to stay a dead end rather than a guess: treating an unrecognised
     * reason as `duplicateName` is the exact bug this string exists to avoid, since
     * acknowledging a refusal that was never the name collision only loops.
     */
    moveRefused:
      'The server refused to file this model there, for a reason this app does not recognise. Nothing changed — try again, or pick a different category.',
    /**
     * `409 crossLibrary` — the target category belongs to a different library. A dead end
     * for this attempt: acknowledging cannot move the category into this library, so the
     * fix is a different target, not a retry of the same one.
     */
    crossLibraryRefusal:
      "That category belongs to a different library. A model can only be filed under a category in its own library — pick one from this library's tree.",
    /**
     * `409 noSuchFolder` — the target category no longer exists, e.g. a sidebar that has
     * not noticed a delete made elsewhere. Also a dead end: there is nowhere to file the
     * model until the tree is reloaded and a real target is picked from it.
     */
    noSuchFolderRefusal:
      'That category no longer exists, so there is nowhere to file this model. Reload the category tree and try again.',
    duplicateTitle: (name: string) => `“${name}” is already in this folder`,
    duplicateBody: 'Two models can share a name — they are told apart by where they came from.',
    duplicateConfirm: 'Move anyway',
    deleteAction: 'Delete',
    deleteFor: (name: string) => `Delete the category ${name}`,
    deleteTitle: (name: string) => `Delete ${name}?`,
    /**
     * What a delete actually does, counted — in models AND in subcategories, because
     * `soft_delete_subtree` marks every descendant folder deleted as well. Naming only the
     * models is how a category holding twelve empty subcategories used to read "No models
     * are inside it. Nothing is removed…" and then take twelve rows off the sidebar. The
     * requirement is that a destructive confirmation names what it affects, and half of
     * what this one affects is the tree itself.
     *
     * `FolderNode.partCount` is subtree-inclusive for the same reason; the subcategory
     * count is the caller's own walk of the tree it already has, excluding the category
     * being deleted, so the two numbers count different things and neither counts twice.
     *
     * Deliberately NOT the plan's "The N models inside will be moved to deleted": that
     * reads at a glance as files being relocated on disk, which is the exact confusion
     * this product forbids, and it is ungrammatical at one. Nothing moves and nothing is
     * erased — the rows are marked deleted and the grid stops showing them.
     *
     * It also drops the plan's "and you can undo this". A soft delete is recoverable in
     * the database, but this slice ships no control that recovers one, and copy that
     * promises an action the app does not have is the same class of lie as a mesh figure
     * presented as analytic.
     */
    deleteBody: (parts: number, subcategories: number = 0) => {
      const inside =
        parts === 0 && subcategories === 0
          ? 'Nothing is inside it — no models, no subcategories.'
          : `${opensSentence(countedModels(parts))} and ${countedSubcategories(subcategories)} are inside it, counting every level.`
      const subcategoriesGo =
        subcategories === 0
          ? ''
          : subcategories === 1
            ? ', and hides the subcategory under it'
            : ', and hides the subcategories under it'
      const action =
        parts === 0
          ? subcategories === 0
            ? 'Deleting hides this category'
            : subcategories === 1
              ? 'Deleting hides this category and the subcategory under it'
              : 'Deleting hides this category and the subcategories under it'
          : parts === 1
            ? `Deleting marks that model deleted and hides it from the grid${subcategoriesGo}`
            : `Deleting marks them deleted and hides them from the grid${subcategoriesGo}`
      return `${inside} ${action}; nothing is removed from your storage folder and no file moves on disk.`
    },
    deleteConfirm: 'Delete category',
    deleteFailed:
      'Could not delete this category. Check that the api service is running, then try again.',
    /**
     * `404 noSuchFolder` — the category was already deleted, by someone else or in another
     * tab, and this sidebar had not noticed. Not `deleteFailed`: the service answered, and
     * "try again" could only ask about the same missing category a second time. The tree is
     * refetched underneath this note, so the row it names is on its way out as it is read.
     */
    deleteGone:
      'That category is already gone — it was deleted somewhere else while this list was on screen. Nothing changed just now, and the list has been reloaded.',
    /**
     * The counts the confirmation showed were read when it opened; the ones here came back
     * with the delete. They disagree when the library moved underneath the open dialog, and
     * a confirmation that named a number owes the user the real one when it turns out to
     * have been a different number.
     */
    deleteCountsDiffered: (parts: number, subcategories: number) =>
      `The category changed while the confirmation was open: ${countedModels(parts)} and ${countedSubcategories(subcategories)} were hidden, not the numbers shown. Nothing was removed from your storage folder.`,
    cancel: 'Cancel',
    /**
     * Not "Show in folder": that is the reveal-in-Finder idiom, and every OS that has it
     * opens a file manager with the file selected. This control expands a panel of text.
     * Borrowing the label promises the gesture, and the body copy immediately below it
     * exists to explain that the gesture is not available — a label should not need the
     * paragraph under it to take back what it said.
     */
    showInFolder: 'Show storage path',
    showInFolderFor: (name: string) => `Show the storage path for ${name}`,
    /**
     * The path, and why it is a path rather than a button. No browser opens a host file
     * manager — `file://` links are blocked everywhere — so this shows where to look
     * instead of pretending to a capability the web build does not have. A native reveal
     * belongs to the Tauri shell.
     *
     * Two versions, because there are two truths to tell. With `LAPIDARY_HOST_STORAGE_ROOT`
     * set to an absolute path the server can say where the store really is, and what is on
     * screen is a path that will open — so the copy stops apologising and just says to
     * paste it. Without it the path is store-relative, the user has to know where their own
     * store is, and pretending otherwise would be the confidently-wrong answer this whole
     * feature refuses to give.
     */
    directoryHint:
      'A browser cannot open a file manager, so this is the path rather than a button. Copy it and open it where your files are.',
    directoryHintAbsolute:
      'A browser cannot open a file manager, so this is the path rather than a button. Copy it and paste it into yours.',
    /**
     * Shown under a store-relative path, once, where a person is looking at exactly the
     * thing it would fix. Names the variable rather than describing it: somebody editing
     * `deploy/.env` needs the string to search for.
     */
    directoryPartial:
      'This is the path inside your storage folder. Set LAPIDARY_STORAGE_ROOT to an absolute path in deploy/.env and the full path appears here instead.',
    /**
     * `directory` is null: this model predates the folder layout and still lives in the
     * shared store. Wording taken from the move route's own refusal, so the two places a
     * user meets this state say the same thing.
     */
    directoryPending:
      'This model has not finished moving into the new storage layout yet, so it has no folder of its own to show. It gets one when the storage migration finishes.',
    /** The same state, where it stops a move rather than a path from being shown. */
    notMigrated:
      'This model has not finished moving into the new storage layout yet. Wait for the storage migration to finish, then try again.',
    copyPath: 'Copy path',

    /**
     * Create and rename. Both routes shipped with the folder tree and neither had a control
     * until now, which is why `FolderTree` could show you a category and delete it but not
     * make one.
     */
    newCategory: 'New category',
    /**
     * Where it will go, said in the title rather than left to be inferred from the sidebar
     * selection. The button is one control whose target moves with that selection, so the
     * dialog owes the user the answer before they type into it.
     */
    createTitle: (parent: string | null) =>
      parent === null ? 'New category' : `New category inside ${parent}`,
    createLabel: 'Category name',
    createConfirm: 'Create',
    createFailed:
      'The category could not be created. Check that the api service is running, then try again.',

    renameAction: 'Rename',
    renameFor: (name: string) => `Rename ${name}`,
    renameTitle: (name: string) => `Rename ${name}`,
    renameConfirm: 'Rename',
    renameFailed:
      'The category could not be renamed. Check that the api service is running, then try again.',
    /**
     * The one thing a rename dialog must say, and the reason it is a sentence rather than a
     * footnote: `DATA.md` §1.1 makes the directory a category's *address*, allocated once at
     * creation, and the name only its label. The whole point of the store's layout is that
     * you can open it in a file manager — so somebody who fixes `Terain` to `Terrain` here
     * and then goes looking for a `Terrain` folder needs to have been told, once, that they
     * will not find one.
     */
    renameKeepsDirectory: (slug: string) =>
      `The folder on disk stays ${slug}. A category keeps the folder it was created in, so renaming changes what you see here and moves nothing on your drive.`,

    /**
     * The refusals a create or a rename can answer with. Mapped from `reason` rather than
     * rendered from the server's prose, the way the move's four already are — the wire
     * carries the reason, this file carries the wording.
     */
    nameTaken:
      'A category here already has that name. Pick another, or rename the one that has it.',
    /**
     * Two ways to reach this and the message covers both, because the user can go and look
     * at the folder either way: names that differ only in characters a filesystem cannot
     * store, and a sibling that was renamed away from this name and kept its folder.
     */
    slugTaken:
      'Another category here already occupies the folder that name needs — either the two names differ only in characters a filesystem cannot store, or that category was renamed and kept the folder it was created in. Pick a different name.',
    emptyName: 'A category needs a name. Type one and try again.',
    writeGone:
      'That category is no longer there — it was deleted somewhere else while this was open. Reload and try again.',
    writeUnknown:
      'The server refused that, and did not say why. Reload the tree and try again; if it keeps happening, the api service log has the reason.',
  },
  libraries: {
    /**
     * The switcher. Every screen is one library's, and until now that library was a UUID
     * compiled into the client — so this control is what makes the id in `api.ts` a fallback
     * rather than the answer.
     */
    label: 'Library',
    /** How many models are in it, so a switcher says where anything is. */
    option: (name: string, parts: number) =>
      parts === 1 ? `${name} — 1 model` : `${name} — ${parts.toLocaleString('en-US')} models`,
    create: 'New library',
    createTitle: 'New library',
    nameLabel: 'Library name',
    createConfirm: 'Create',
    modeLabel: 'Governance',
    /**
     * Chosen at creation, and switchable one way afterwards: a hobby library can start keeping
     * every change, and nothing switches one back, because a controlled library switched back
     * would hold revisions no screen shows. States and approvals are still Phase 8.
     */
    hobby: 'Hobby — no revisions or approvals',
    controlled: 'Controlled — every change kept as a revision',
    /** The one-way switch, named for what it does rather than for the mode it sets. */
    makeControlled: 'Keep every change…',
    makeControlledTitle: 'Keep every change in this library?',
    makeControlledBody: (name: string) =>
      `From now on, when a file in ${name} changes, the change is kept as a new revision and the previous file is kept beside it. This cannot be switched back.`,
    makeControlledConfirm: 'Keep every change',
    makeControlledFailed:
      'The library was not switched. Check that the api service is running, then try again.',
    createFailed:
      'The library could not be created. Check that the api service is running, then try again.',
    refusedWithoutReason:
      'That library was refused and the reason did not come back. Try a different name.',
    failed: 'Could not read the list of libraries. Check that the api service is running.',
  },
  grid: {
    /**
     * How many cards a page asks for, and how tightly they pack. Remembered per library,
     * in this browser — the honest scope, and what `FEATURES.md` says now.
     */
    pageSize: 'Cards per page',
    pageSizeOption: (size: number) => `${size.toLocaleString('en-US')} per page`,
    density: 'Card size',
    comfortable: 'Comfortable',
    compact: 'Compact',
    /**
     * The grid's order. Every option names its end, because "Volume" alone does not say
     * whether the largest part comes first. Parts without the figure — an open mesh has no
     * volume — come last, which the grid shows and the options need not say.
     */
    sort: 'Order',
    sortOption: {
      newest: 'Newest first',
      volume: 'Largest volume first',
      surface_area: 'Largest surface area first',
      longest_side: 'Longest side first',
      triangles: 'Most triangles first',
    },
    /** Under the order while a search runs, which a search does not use. */
    sortWhileSearching: 'Search results are in order of relevance.',
  },
  search: {
    label: 'Search this library',
    /**
     * Names what the query actually reaches, which changed when `0020` indexed
     * `source_path`: the filename a model arrived as, and the directory holding it, are
     * both searchable now. A placeholder that still said "name or part number" would be
     * hiding the one field a person is most likely to remember about a download.
     *
     * It does not say "tag" or "creator" the way `v2`'s does. There is no tag table and no
     * user table — a placeholder promising either would be a search box that answers
     * nothing for two of the four things it names.
     */
    placeholder: 'Search by name, part number or file path',
    /**
     * The chip that appears when a search runs inside a selected category.
     *
     * It is a **disclosure**, not a control that narrows: the sidebar has already narrowed
     * the grid, and a search that quietly kept that narrowing without saying so is how
     * somebody concludes a part is missing. Dismissing it widens to the whole library and
     * keeps the query.
     */
    inCategory: (name: string) => `in ${name}`,
    /**
     * When the category's name is not known yet — it comes from the folder tree, a different
     * query from the grid's, so there is a window where the chip must exist and cannot name
     * anything. The first version fell back to `folders.title` and rendered "in Categories",
     * which is the sidebar's heading and means nothing here.
     */
    inThisCategory: 'in this category',
    widen: 'Search the whole library instead',
    /**
     * Zero results, and deliberately not `emptyLibrary.body` — "This library is empty. Drop
     * a folder of models above" is false and alarming over a library of 1,700 parts, and the
     * user did not empty anything, they typed something.
     */
    noMatches: (query: string) => `Nothing matches ${query}.`,
    /**
     * The heading over a search that found nothing.
     *
     * Not `emptyLibrary.categoryTitle`, which is what shipped: "Nothing filed here yet"
     * told somebody with no category selected that their library was empty, in the exact
     * moment they had searched for a part they knew was in it.
     */
    noMatchesTitle: 'No matching parts',
    /** What search actually covers, said once rather than left to be inferred. */
    scope: 'Search covers part names and part numbers.',
    clear: 'Clear the search',
    /**
     * The same, narrowed — and this is the one that matters. A query that finds nothing
     * *inside a category* must say the search was narrowed, or the reasonable conclusion is
     * that the part is not in the library at all.
     */
    noMatchesInCategory: (query: string, category: string) =>
      `Nothing in ${category} matches ${query}. It may be filed somewhere else.`,
    /**
     * Under two characters a trigram index cannot be used at all — a trigram is three
     * characters — so the query would be a sequential scan by construction. The box accepts
     * the keystroke and simply does not run yet.
     */
    keepTyping: 'Keep typing — searches start at two characters.',
  },
  images: {
    /**
     * A photograph shows what a render cannot: the finish, the colour, the thing next to a
     * hand. It sits above the render rather than replacing it — the generated view is still
     * the honest picture of the geometry, and `part_image`'s ordering keeps both.
     */
    title: 'Pictures',
    /**
     * Same reason as `sources.failed`: an empty gallery and an unknown one are different.
     *
     * `galleryFailed` and not `failed`, which this key was first — a duplicate object key
     * TypeScript rejected and no render test could have: `images.failed` already exists a
     * few hundred lines down and means a picture that could not be *stored*. The later
     * literal wins at runtime, so the section would have rendered an upload error for a
     * read failure while every assertion comparing the two through `strings` agreed.
     */
    galleryFailed: 'Could not load the pictures for this part.',
    add: 'Add a picture',
    adding: 'Storing…',
    /** Named alt text, because "image" tells a screen reader nothing it did not know. */
    alt: (name: string, index: number) => `Picture ${index + 1} of ${name}`,
    /**
     * The resize is otherwise invisible. Somebody who attached a 4000-pixel photograph is
     * owed the sentence saying what is now on the card — the server answers with the size it
     * stored, so this is a fact rather than a guess.
     */
    resized: (width: number, height: number) =>
      `Stored at ${width}×${height}. Larger pictures are scaled down so a page of cards stays quick to load.`,
    /**
     * A refusal the server did not explain. It always does explain — every `ImageError`
     * carries a sentence — so this covers a body that could not be read rather than a
     * refusal without a reason.
     */
    refusedWithoutReason:
      'That picture was refused and the reason did not come back. Check that it is a PNG, JPEG or WebP under 10 MB, then try again.',
    failed:
      'The picture could not be stored. Check that the api service is running, then try again.',
    /** Where a fetched image came from, so a chosen picture reads differently from a pulled one. */
    from: (url: string) => `From ${url}`,
    /**
     * The other way in. Named for what it does rather than "paste a link", because the
     * distinction that matters is that the picture is *copied* — the server fetches it once
     * and stores it, so it keeps working after the page it came from changes, and looking at
     * a card never sends a request to somebody else's server.
     */
    addFromUrl: 'Add from a link',
    fetch: 'Fetch',
    fetching: 'Fetching…',
    cancelUrl: 'Cancel',
    urlLabel: 'Address of the picture',
    urlPlaceholder: 'https://example.com/bracket.jpg',
    /**
     * The framing controls. `cover` and `contain` are CSS's names and the database's, and
     * deliberately not the labels: "Fill the frame" and "Show the whole picture" say what
     * happens, where `cover` says nothing to somebody who has not written CSS.
     */
    fitFill: 'Fill the frame',
    fitWhole: 'Show the whole picture',
    /**
     * A re-frame the server refused. The picture only changes once the server accepts, so
     * without this line a failure is a control that silently does nothing. Short, because
     * it sits under a tile a quarter of the width of this sentence.
     */
    reframeFailed: 'Could not change the framing. Try again.',
    /** What clicking the picture does, for a screen reader that cannot see the crop. */
    focusLabel: (label: string) =>
      `${label} — click or use the arrow keys to choose what stays in frame`,
  },
  /**
   * What a CAD file specifies about the part's sizes and form. These are a designer's values, so
   * they are said as specified rather than as measured, and carry no ≈.
   */
  pmi: {
    title: 'Dimensions and tolerances',
    note: 'As the file specifies them: the designer’s values, not measurements.',
    failed: 'Could not load the dimensions and tolerances. Reload the page to try again.',
    /** A size and its bounds, e.g. `⌀22 mm +0.05 / 0`. */
    dimension: (kind: string, value: number, upper: number | null, lower: number | null) => {
      const symbol: Record<string, string> = {
        diameter: '⌀',
        radius: 'R',
        spherical_diameter: 'S⌀',
        spherical_radius: 'SR',
        angle: '∠',
      }
      const unit = kind === 'angle' ? '°' : ' mm'
      const size = `${symbol[kind] ?? ''}${value.toLocaleString('en-US', { maximumFractionDigits: 4 })}${unit}`
      if (upper === null || lower === null) return size
      const bound = (deviation: number) =>
        deviation === 0 ? '0' : `${deviation > 0 ? '+' : '−'}${Math.abs(deviation).toLocaleString('en-US', { maximumFractionDigits: 4 })}`
      return `${size} ${bound(upper)} / ${bound(lower)}`
    },
    /** A geometric tolerance by its ISO 1101 symbol and name, its zone, and the datums it is measured from. */
    tolerance: (kind: string, value: number, datums: readonly string[]) => {
      const named: Record<string, [string, string]> = {
        flatness: ['⏥', 'Flatness'],
        straightness: ['⏤', 'Straightness'],
        circularity: ['○', 'Circularity'],
        cylindricity: ['⌭', 'Cylindricity'],
        profile_of_line: ['⌒', 'Profile of a line'],
        profile_of_surface: ['⌓', 'Profile of a surface'],
        parallelism: ['∥', 'Parallelism'],
        perpendicularity: ['⟂', 'Perpendicularity'],
        angularity: ['∠', 'Angularity'],
        position: ['⌖', 'Position'],
        concentricity: ['◎', 'Concentricity'],
        coaxiality: ['◎', 'Coaxiality'],
        symmetry: ['⌯', 'Symmetry'],
        circular_runout: ['↗', 'Circular run-out'],
        total_runout: ['⌰', 'Total run-out'],
      }
      const [symbol, name] = named[kind] ?? ['', 'Tolerance']
      const zone = `${value.toLocaleString('en-US', { maximumFractionDigits: 4 })} mm`
      const from = datums.length === 0 ? '' : ` to ${datums.join(', ')}`
      return `${symbol} ${name} ${zone}${from}`.trim()
    },
    datum: (name: string) => `Datum ${name}`,
    /** The toggle beside the list that draws each annotation beside its face in the 3D view. */
    showInView: 'Show in the 3D view',
    /** Beside an annotation the view cannot place, once the list is shown in the view. */
    notDrawn: (wholePart: boolean) =>
      wholePart
        ? 'not in the view: it applies to the whole part'
        : 'not in the view: its face is not a plane, cylinder, cone, sphere or torus',
    /** In place of those reasons when the faces themselves could not be read, so no face is blamed. */
    facesUnread: 'The view draws none of these: the faces of this part could not be read. Reload the page to try again.',
    /** Where an annotation applies: the kind of face measurement reads there, or the whole part. */
    face: (surface: string | null) =>
      surface === null
        ? 'the whole part'
        : ({
            plane: 'a planar face',
            cylinder: 'a cylindrical face',
            cone: 'a conical face',
            sphere: 'a spherical face',
            torus: 'a toroidal face',
          }[surface] ?? 'a face'),
  },
  tags: {
    /** What people call a part beyond its name: a project, a use, a shelf. */
    title: 'Tags',
    field: 'New tag',
    add: 'Add tag',
    saving: 'Saving…',
    /** The remove button's whole name, so a screen reader hears which tag goes. */
    remove: (tag: string) => `Remove tag ${tag}`,
    /** A refusal that arrived without a sentence of its own. */
    refusedWithoutReason: 'Could not save these tags. Reload the part and try again.',
  },
  materials: {
    /** What a part is made of: typed here, or read off its CAD file until someone types them. */
    title: 'Materials',
    field: 'New material',
    add: 'Add material',
    saving: 'Saving…',
    /** The remove button's whole name, so a screen reader hears which material goes. */
    remove: (material: string) => `Remove material ${material}`,
    /** Beside materials nobody typed. */
    fromFile: 'As the file states. A list you change here is kept instead, until you remove every material.',
    /** A refusal that arrived without a sentence of its own. */
    refusedWithoutReason: 'Could not save these materials. Reload the part and try again.',
  },
  fields: {
    /** A library's own named values on its parts: a supplier, a stock count. */
    title: 'Fields',
    menu: 'Fields…',
    dialogTitle: 'Fields in this library',
    none: 'No fields yet. Add one to give parts a supplier, a stock count or anything else of your own.',
    label: 'Label',
    key: 'Key',
    keyDetail: 'Lowercase letters, digits and underscores. A key cannot be renamed later.',
    kind: 'Holds',
    text: 'Text',
    number: 'A number',
    choice: 'One of a list',
    options: 'Options, one per line',
    offered: 'Offer as a grid filter',
    add: 'Add field',
    save: 'Save',
    saving: 'Saving…',
    close: 'Close',
    remove: (label: string) => `Remove the field ${label}`,
    removeNote: 'Removing a field keeps the values parts already hold.',
    /** Values a part holds for fields this library no longer defines. */
    orphaned: 'No longer fields in this library',
    unset: 'Not set',
    refusedWithoutReason: 'Could not save this field. Reload and try again.',
    loadFailed: 'Could not load this library’s fields. Reload to try again.',
    /** A filter's value box, named for the field it filters. */
    filterValue: (label: string) => `${label} is`,
    /** A number field's two range boxes, named for the field, each showing its end as a hint. */
    filterFrom: (label: string) => `${label} from`,
    filterTo: (label: string) => `${label} to`,
    /** A choice's option as its filter button names it, with how many of the grid's parts hold it. */
    choiceOption: (option: string, count: number | null) =>
      count === null ? option : `${option}, ${count.toLocaleString('en-US')} ${count === 1 ? 'part' : 'parts'}`,
    from: 'from',
    to: 'to',
    filterApply: 'Filter',
    filterClear: (label: string) => `Clear the ${label} filter`,
  },
  densities: {
    /** Each material's density in a library, typed by a person: what a part's mass is worked out from. */
    menu: 'Densities…',
    dialogTitle: 'Densities in this library',
    intro:
      'A part’s mass is its volume times the density of its material. Densities are typed here, not measured, so a mass is always approximate.',
    none: 'No part in this library has a material yet. Give a part its material on its page, then set a density here.',
    /** A density box, named for its material and the unit it is typed in. */
    field: (material: string) => `${material}, g/cm³`,
    save: 'Save',
    saving: 'Saving…',
    remove: (material: string) => `Remove the density of ${material}`,
    unreadable: 'Type the density as a number of grams per cubic centimetre, such as 7.85 for steel.',
    /** A density the server refused as out of range, said in the unit this dialog is typed in. */
    outOfRange: 'A density is a number of grams per cubic centimetre above 0 and below 25, such as 7.85 for steel. Type it again.',
    close: 'Close',
    loadFailed: 'Could not load this library’s densities. Reload to try again.',
    refusedWithoutReason: 'Could not save this density. Reload and try again.',
  },
  sources: {
    /**
     * Where the model came from. The licence is the field this exists for: `docs/DATA.md`
     * is emphatic that half of hobbyist STL libraries are non-commercial and somebody
     * selling prints needs to see that before they print.
     */
    title: 'Where this came from',
    /**
     * A fetch that failed renders this instead of an empty list. `sources.data ?? []`
     * reads a network failure as "nothing recorded", which is the one thing this section
     * must never say about a part whose licence it could not load.
     */
    failed: 'Could not load where this came from.',
    add: 'Record a source',
    save: 'Save',
    saving: 'Saving…',
    cancel: 'Cancel',
    url: 'Link',
    titleField: 'Title',
    vendor: 'Vendor',
    /** Whatever the seller calls it — an SKU, a Thingiverse id, a catalogue number. */
    externalId: 'Their reference',
    license: 'Licence',
    priceField: 'Price',
    currency: 'Currency',
    /** A source with nothing to show for itself but a row. */
    untitled: 'Recorded source',
    /**
     * Minor units to money. `Intl.NumberFormat` does the placement, the symbol and the
     * separators, all of which differ by currency and none of which is worth hand-rolling —
     * and `en-US` is the locale everything else in this file already uses.
     */
    price: (minor: number, currency: string) =>
      new Intl.NumberFormat('en-US', { style: 'currency', currency }).format(minor / 100),
    refusedWithoutReason:
      'That source was not recorded and the reason did not come back. Check that the api service is running, then try again.',
  },

  dialog: {
    /**
     * The mouse's way out. Escape has always worked and was signposted nowhere, which made
     * every dialog in the application keyboard-only to dismiss — and unusable on touch.
     */
    close: 'Close',
  },

  /** The 3D view in the quick look and on the part's page. */
  viewer: {
    label: (name: string) => `3D view of ${name}`,
    /** Under the rendered preview where the browser cannot draw 3D, so its absence is explained. */
    noWebGL: 'This browser cannot draw the 3D view here, so this is the rendered preview.',
    refining: 'Loading more detail…',
    failed: 'The 3D view could not load. The rendered preview is shown instead.',
    ghostFailed: 'The earlier revision’s mesh could not load, so no ghost is drawn.',
  },
  /**
   * The measuring tools under the 3D view. A value is exact only where it was read from an
   * analytic CAD entity — a cylinder's radius, a plane's equation — and every other one carries
   * `detail.approximate` through `Figure`, the same mark the part's own figures carry.
   */
  measure: {
    label: 'Measure',
    tools: {
      distance: 'Point to point',
      edge: 'Edge length',
      diameter: 'Diameter',
      angle: 'Angle',
      wall: 'Wall thickness',
    },
    prompts: {
      distance: 'Click two points on the part.',
      edge: 'Click the corner at each end of the edge.',
      diameter: 'Click a round face, or three points around a round edge.',
      angle: 'Click two faces, or a cone for its included angle.',
      wall: 'Click a wall. Its thickness is measured straight through it.',
    },
    /** Picks wait for the finest mesh, so no value is read off a coarser one than the part has. */
    loading: 'Loading the full-detail mesh to measure on…',
    unavailable:
      'The full-detail mesh could not be built, so this part cannot be measured here. Reopen the part to try again.',
    noWall: 'Nothing is opposite that point, so there is no wall to measure. The mesh may be open there.',
    millimetres: (value: number) => `${fixed(value)} mm`,
    degrees: (value: number) => `${fixed(value)}°`,
  },
  /**
   * The section plane, beside the measuring tools. A cut changes what is drawn and what a click can
   * meet, never a reading taken from what is left, and it is open: it shows the part's inside
   * surfaces rather than a filled face.
   */
  section: {
    label: 'Section',
    off: 'Off',
    axes: { x: 'X', y: 'Y', z: 'Z' },
    position: 'Where the cut is',
    flip: 'Flip',
    /** Beside a cut through a mesh measured open: it has no inside, so nothing is filled. */
    open: 'This mesh is open, so the cut shows no filled face.',
    /** Beside a cut through a mesh nobody measured: it might be open, so nothing is filled. */
    unknown: 'Whether this mesh is closed was not measured, so the cut shows no filled face.',
  },
  /** Drawing an assembly's parts apart, under the section controls. */
  explode: {
    label: 'Explode',
    /** In place of the measuring line while the parts are apart. */
    measuringOff:
      'Measuring is off, and so are PMI labels, while the parts are apart: a distance between moved parts is not one on the assembly, and a label would stay where its face was. Slide back to measure.',
  },
  quickLook: {
    /**
     * The card opens a panel rather than navigating, because scanning a library means
     * looking at one part and then the next one — and a round trip through a full page and
     * the back button for each of them is the thing that makes a library tiring to go
     * through.
     */
    openFor: (name: string) => `Open ${name}`,
    /**
     * Out to the real page. The dialog is for a look; the page is where the controls that
     * change something live, and it is a URL that can be shared and bookmarked.
     */
    fullPage: 'Open the full page',
    /** Where the stage is in the parts on screen: `3 of 40`. */
    position: (at: number, of: number) => `${at.toLocaleString('en-US')} of ${of.toLocaleString('en-US')}`,
    /** Under the stage's figures, once: the keys are the fastest way through a library and nothing else says so. */
    steps: 'Left and right arrows step through the parts.',
    loading: 'Loading…',
    failed:
      'Could not open this part. Check that the api service is running, then try again.',
  },
  emptyLibrary: {
    title: 'Nothing here yet',
    body:
      'This library is empty. Drop a folder of models above to add them, or scan the directory mounted on the server — either way, every model found appears here.',
    /**
     * An empty *category* is not an empty library, and saying so was a lie the grid could
     * always tell — a scan that found an empty directory made one — but which only became
     * easy to reach when categories became something a person could create. Somebody makes
     * `Workholding`, looks at it, and is told the library holding their 1,700 models is
     * empty.
     *
     * Named, because the difference is the whole point: a user who has just filed nothing
     * into a category they made needs to know the models are still where they were, not
     * that they are gone.
     */
    categoryTitle: 'Nothing filed here yet',
    /**
     * The name is optional because the tree it comes from is a second query, and a page
     * that has the grid's answer but not the sidebar's must still not claim the library is
     * empty. Naming the category is better and not required to be truthful.
     */
    categoryBody: (name: string | null) =>
      `No models are in ${name ?? 'this category'} yet. Drag a card onto it in the sidebar, or use a card's "Move to…" button — the rest of the library is still under All models.`,
  },
  /**
   * Sharing with people you know (S1b): this installation's device id and name, and the people it is paired
   * with. "Shared libraries" is the owner's word for the area; the code says `peer`.
   */
  sharing: {
    title: 'Shared libraries',
    lead: 'Share with people you know. Each of you pastes the other’s device id and address here, and your machines talk to each other directly — nothing passes through anybody else’s server.',
    loading: 'Loading…',
    loadFailed: 'Could not load sharing on this installation. Reload to try again.',
    thisInstallation: 'This installation',
    /** No device id yet: the peer role has never run here. */
    off: 'Sharing is switched off here. Start the sharing service with deploy/compose.sharing.yaml, and this installation’s device id appears here for you to give to people.',
    deviceId: 'Device id',
    copy: 'Copy',
    copied: 'Copied',
    nameField: 'The name people you share with see',
    namePlaceholder: 'Furkan’s workbench',
    saveName: 'Save name',
    savingName: 'Saving…',
    people: 'People you share with',
    /** Removal is soft, and this is where a person learns that before they click. */
    removeNote: 'Removing somebody stops sharing with them straight away. Their entry is kept, and pairing with them again brings it back.',
    none: 'Nobody yet. Pair with somebody by pasting their device id and address.',
    pairDeviceId: 'Their device id',
    pairAddress: 'Where to reach them',
    addressPlaceholder: '192.168.1.24:8082',
    pair: 'Pair',
    pairing: 'Pairing…',
    refusedWithoutReason: 'Could not save that. Reload and try again.',
    online: 'Online',
    /** Paired, and no hello has been answered yet. */
    notReached: 'Not reached yet',
    lastSeen: (at: string) =>
      `Offline — last seen ${new Date(at).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' })}`,
    /** Shown in place of a name until their hello has given one. */
    unnamed: 'No name given yet',
    removeButton: 'Remove',
    removeLabel: (who: string) => `Stop sharing with ${who}`,
    removing: 'Removing…',
    /** Sharing a category (S2a): the dialog that shows what would be offered before anything is. */
    shareTitle: (name: string) => `Share ${name}`,
    shareBody: (parts: number) =>
      parts === 1
        ? '1 part in this category and everything under it will be offered to everyone you are paired with, and so will parts you file here later.'
        : `${parts} parts in this category and everything under it will be offered to everyone you are paired with, and so will parts you file here later.`,
    shareUnrecorded: (count: number) =>
      count === 1 ? '1 part has no licence recorded.' : `${count} parts have no licence recorded.`,
    shareNonCommercial: (count: number) =>
      `${count === 1 ? '1 part is' : `${count} parts are`} licensed for non-commercial use only. The people you share with see each part's licence.`,
    shareLicencesClear: 'Every part has a licence recorded, and none is licensed for non-commercial use only.',
    shareCounting: 'Counting what this would offer…',
    shareCountFailed: 'Could not count what this category holds. Close this and try again.',
    shareConfirm: 'Share',
    sharingNow: 'Sharing…',
    shareCancel: 'Cancel',
    shareFailed: 'Could not share this category. Close this and try again.',
    /** The sharing page's list of what this installation offers. */
    ownShares: 'What this installation shares',
    ownSharesNone: 'Nothing yet. Share a category from the tree beside a library, and it appears here.',
    ownShareParts: (count: number) => (count === 1 ? '1 part offered' : `${count} parts offered`),
    stopNote: 'Stopping takes a category away from everyone you are paired with at once. Nothing in it is deleted.',
    stopSharing: 'Stop sharing',
    stopSharingLabel: (name: string) => `Stop sharing ${name}`,
    stopping: 'Stopping…',
    /** Under each person on the sharing page: what they share, each a link to it. */
    theirShares: 'What they share',
    theirSharesNone: 'Nothing shared with you yet.',
    theirShareParts: (name: string, count: number) =>
      count === 1 ? `${name}, 1 part` : `${name}, ${count} parts offered`,
    /** A shared library: somebody else's category, read from the mirror, so it browses while they are away. */
    libraryTitle: (name: string) => `${name} — Shared libraries — Lapidary`,
    backToSharing: 'Back to shared libraries',
    librarySharedBy: (sharer: string, count: number) =>
      count === 1 ? `Shared by ${sharer}, 1 part` : `Shared by ${sharer}, ${count} parts offered`,
    unnamedSharer: 'somebody who has not given a name',
    librarySynced: (at: string) =>
      `Last read ${new Date(at).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' })}`,
    libraryNotReadYet:
      'Not read yet. It is read the next time their machine answers, and appears here once it has been.',
    /** Parts leave this list when the sharer stops offering them; nothing of this installation's goes with them. */
    libraryLead:
      'This is what they offer. A part they stop offering leaves this list; nothing of yours goes with it.',
    libraryGone:
      'This shared library is not here any more: its sharer stopped offering it, or you removed them.',
    libraryLoadFailed: 'Could not load this shared library. Reload to try again.',
    libraryEmpty: 'Nothing in it at the moment.',
    noLicence: 'No licence recorded',
    licences: (list: string) => `Licence: ${list}`,
    partKind: (format: string | null, size: number | null) =>
      [format?.toUpperCase(), size === null ? null : bytes(size)].filter((piece) => piece).join(' · '),
    showMore: 'Show more',
    showingMore: 'Loading…',
    /** Pulling a shared library (S3) into one of this installation's. */
    pullInto: 'Pull into',
    pullAll: 'Pull all',
    pullStarting: 'Starting…',
    pullFailed: 'Could not start the pull. Reload and try again.',
    pullNote:
      'Pulled parts land under Shared, in a category named for who shares them, with their licences. Parts you already pulled are not fetched again.',
    pullQueued: 'Waiting for the sharing service to start fetching…',
    pullFetching: (done: number, total: number, bytesDone: number, bytesTotal: number) =>
      `Fetching ${done} of ${total} files — ${bytes(bytesDone)} of ${bytes(bytesTotal)}`,
    pullImporting: (settled: number, total: number) =>
      total === 0 ? 'Importing what was fetched…' : `Importing — ${settled} of ${total} jobs finished`,
    pullDone: (files: number) =>
      files === 0
        ? 'Pulled. This library already held every file, so nothing was fetched.'
        : files === 1
          ? 'Pulled 1 file.'
          : `Pulled ${files} files.`,
    pullStopped: (why: string) => `The pull stopped: ${why}`,
    /** Unfinished, with the reason the last attempt stopped: it is tried again on its own. */
    pullRetrying: (why: string) => `Trying again shortly. The last attempt stopped: ${why}`,
    /** Asking first (S4). */
    pullWaiting: 'Waiting for the sharer to let you pull this. Asked again every few seconds.',
    pullPaused: 'Paused. What was fetched so far is kept, and resuming carries on from it.',
    pause: 'Pause',
    resume: 'Resume',
    pullControlFailed: 'Could not change the pull. Reload and try again.',
    askFirstLabel: 'Ask me before anyone pulls its files',
    askFirstNote: 'Everyone you are paired with still sees what it holds. Files go only to the people you let pull it.',
    asksFirst: 'Asks first',
    requests: 'Asking to pull',
    requestsNote: 'People asking to pull a share that asks first. You can change an answer later.',
    requestsNone: 'Nobody has asked to pull from a share that asks first.',
    requestLine: (who: string, share: string) => `${who} asks to pull ${share}`,
    requestAsked: 'Waiting for your answer',
    requestGranted: 'You let them pull it',
    requestDenied: 'You declined',
    grant: 'Let them pull',
    deny: 'Decline',
    grantLabel: (who: string, share: string) => `Let ${who} pull ${share}`,
    denyLabel: (who: string, share: string) => `Decline ${who}’s request to pull ${share}`,
    requestFailed: 'Could not save your answer. Reload and try again.',
    pulls: 'Your pulls',
    pullLine: (share: string, sharer: string) => `${share}, from ${sharer}`,
    pullFrom: (share: string) => `${share}, from somebody you removed or who gave no name`,
    pullImportingPlain: 'Importing what was fetched…',
  },
} as const
