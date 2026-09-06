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
 * The counted phrases above read mid-sentence as well as at the head of one — "no models"
 * has to stay lowercase where a sentence has already started — so the one place that opens
 * a sentence with one raises its first letter itself.
 */
function opensSentence(clause: string): string {
  return clause.charAt(0).toUpperCase() + clause.slice(1)
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
     * Counted in steps, not in files, and that is the same measurement rule the doc above
     * spells out: one failed `migrate_storage` job is one run over a page of up to 200
     * hashes, so calling it a file would report 200 files as 1. What matters more than the
     * unit is what a migration failure is NOT — nothing was deleted, nothing left the
     * grid, and the models involved still open from where they always were.
     */
    failed: (count: number) =>
      count === 1
        ? '1 step of the move could not be finished. Those files stay where they already were — nothing was removed and every model is still listed.'
        : `${count.toLocaleString('en-US')} steps of the move could not be finished. Those files stay where they already were — nothing was removed and every model is still listed.`,
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
    failed:
      'Could not read what this library occupies. Check that the api service is running, then reload.',
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
    loading: 'Loading categories…',
    failed:
      'Could not load the categories in this library. Check that the api service is running, then reload.',
    empty: 'No categories yet — they appear when you scan a folder.',
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
     */
    directoryHint:
      'A browser cannot open a file manager, so this is the path rather than a button. Copy it and open it where your files are.',
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
  },
  emptyLibrary: {
    title: 'Nothing scanned yet',
    body:
      'This library is empty. Lapidary ingests from a directory mounted on the server, not from files on this machine — scan that directory and every model it finds appears here.',
  },
} as const
