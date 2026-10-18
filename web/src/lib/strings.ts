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
    settings: 'Settings — Lapidary',
    profile: 'Profile — Lapidary',
    dashboard: 'Dashboard — Lapidary',
    shared: 'Shared with me — Lapidary',
    people: 'People — Lapidary',
    setup: 'Set up — Lapidary',
    login: 'Sign in — Lapidary',
  },
  /**
   * The bar above the grid: where you are, what you are looking for, how it is laid out, and
   * what can be done to the library. `v2` draws all of it in one row; the menus are what let
   * it fit, and every control inside them is the same control it was when it had a row of
   * its own.
   */
  /**
   * The Settings page (F3): the keys for the sites a model can be imported from, each checked
   * against its site before anybody relies on it. A saved key is shown by its last four
   * characters and never in full.
   */
  settings: {
    title: 'Settings',
    lead: 'Your keys for the sites Lapidary imports models from, how adding models starts, and the port this server answers on. A key stays on this server: it is shown here by its last four characters, and sent only to its own site.',
    sitesHeading: 'Model sites',
    switchedOff:
      'Importing from model sites is switched off on this install (LAPIDARY_IMPORT_LINKS=false). Whoever runs the server can turn it on in deploy/.env.',
    loading: 'Loading settings…',
    failed: 'Could not load the settings. Check that the server is running, then reload.',
    noKeyNeeded: 'Free models import without a key.',
    keyLabel: (site: string) => `${site} key`,
    /** Where each site's key comes from, said once beside the field. */
    keyHelp: {
      thingiverse: 'An app token: create an app at thingiverse.com/apps/create and copy its token.',
      myminifactory:
        'Your API key (myminifactory.com/settings/developer) or your account’s access token (OAuth). Either gives a model’s pictures, tags, licence and creator; only the token downloads files, including what your account has bought.',
    } as Record<string, string>,
    saved: (hint: string) => `A key ending ${hint} is saved.`,
    noneSaved: 'No key saved.',
    lastCheck: (ok: boolean, when: string) =>
      `${ok ? 'Worked' : 'Did not work'} when checked ${new Date(when).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' })}.`,
    save: 'Save',
    check: 'Check',
    checkSaved: 'Check the saved key',
    checkSite: 'Check that it answers',
    checking: 'Checking…',
    remove: 'Remove',
    saveFailed: 'The key was not saved. Check that the server is running, then try again.',
    checkFailed: 'The check could not run. Check that the server is running, then try again.',
    serverHeading: 'Server',
    port: 'Port',
    portHint: 'Lapidary moves to the new port with no restart. This page follows it; other open tabs keep working for a minute.',
    portLocked: (by: string) => `Set by ${by} in deploy/.env. Change it there and run docker compose up -d.`,
    portAdminsOnly: 'Only an admin can change the port.',
    movePort: 'Move',
    moving: (port: number) => `Moving Lapidary to port ${port}…`,
    moveLost: (port: number) =>
      `Lapidary moved to port ${port} but this page could not reach it. Open it on port ${port} yourself.`,
    modelsFolder: 'Models folder',
    modelsFolderHint:
      'Where your models are, and where uploads, imports and removed models go. Choosing another restarts Lapidary on it; models read from the current one must be removed from the library first.',
    modelsFolderLocked: (by: string) => `Set by ${by} in deploy/.env. Change it there and run docker compose up -d.`,
    modelsFolderAdminsOnly: 'Only an admin can choose the models folder.',
    modelsFolderFixed: 'This deployment mounts no folder to choose from, so the models folder is set in deploy/.env.',
    modelsFolderUnknown: 'The models folder in deploy/compose.yaml (./models unless LAPIDARY_MODELS_DIR says otherwise)',
    store: 'Lapidary’s own files',
    storeUnknown: 'storage/ beside deploy/ (LAPIDARY_STORAGE_ROOT in deploy/.env)',
    storeHint: 'Pictures and Lapidary’s copies. Chosen in the setup wizard, and kept where it is after that, so every download keeps working.',
    restarting: (path: string) => `Restarting Lapidary on ${path}…`,
    restartLost: 'Lapidary was asked to restart but has not come back. Check `docker compose logs app`, then reload.',
    importHeading: 'Adding models',
    fetchDetails: 'Fetch a model’s details from its page when I add it with a link',
    fetchDetailsHint:
      'Pictures, tags, licence and creator, from Printables, Thingiverse or MyMiniFactory. Nothing is downloaded; the file is the one in your models folder.',
  },
  /** The models-folder picker (A6): one folder of the machine at a time. */
  folderPicker: {
    choose: 'Choose…',
    modelsTitle: 'Choose your models folder',
    storeTitle: 'Choose where Lapidary keeps its own files',
    use: 'Use this folder',
    up: 'Up one folder',
    cancel: 'Cancel',
    pathLabel: 'Folder path',
    go: 'Go',
    crumbs: 'Path',
    newFolder: 'New folder',
    newName: 'Name of the new folder',
    make: 'Make',
    making: 'Making…',
    loading: 'Reading the folder…',
    empty: 'No folders here.',
    failed: 'Could not read this folder.',
    listLabel: 'Folders',
    open: (name: string) => `Open ${name}`,
  },
  /** The user menu at the header's right (M2): this install's profile, and its settings. */
  user: {
    menu: 'Account',
    profile: 'Profile',
    signOut: 'Sign out',
    people: 'People',
  },
  /** Sharing a model (A4): with everybody on the install, or with chosen people; to look and download. */
  sharing: {
    action: 'Share…',
    title: (n: number) => (n === 1 ? 'Share this model' : `Share ${n.toLocaleString('en-US')} models`),
    lead: 'The people you share with can find it under Shared with me, look at it and download it. Only you can change it.',
    everyone: 'Everybody on this install',
    people: 'People',
    nobodyElse: 'Nobody else has an account yet. An admin adds people under People.',
    loading: 'Loading…',
    save: 'Share',
    saving: 'Sharing…',
    readOnly: (owner: string | null) =>
      owner === null
        ? 'Shared with you: you can look at it and download it. Only its owner can change it.'
        : `Shared with you by ${owner}: you can look at it and download it. Only they can change it.`,
    place: 'Shared with me',
    title_page: 'Shared with me',
    empty: 'Nobody has shared a model with you yet.',
    loadingPage: 'Loading what was shared with you…',
    failed: 'Could not load what was shared with you. Check that the server is running, then reload.',
  },
  /** The admin's accounts page (A4). */
  people: {
    title: 'People',
    lead: 'Everybody with an account on this install. Each person sees their own models and the ones shared with them.',
    adminsOnly: 'Only an admin can add or remove people.',
    you: 'you',
    admin: 'admin',
    addHeading: 'Add a person',
    username: 'Username',
    displayName: 'Display name',
    password: 'First password',
    passwordHint: 'At least 8 characters. Tell them, and they can change it on their Profile page.',
    makeAdmin: 'Admin: can add and remove people',
    add: 'Add',
    adding: 'Adding…',
    added: (name: string) => `${name} can sign in now. They start with an empty library.`,
    reset: 'New password',
    resetPrompt: (name: string) => `A new password for ${name}`,
    resetDone: (name: string) => `${name} has a new password and is signed out everywhere.`,
    remove: 'Remove',
    removeTitle: (name: string) => `Remove ${name}?`,
    removeBody: (name: string) =>
      `${name} can no longer sign in. Their libraries and models become yours, so nothing they added is lost.`,
    removeConfirm: 'Remove',
    loading: 'Loading people…',
    failed: 'Could not load the people on this install. Check that the server is running, then reload.',
  },
  /**
   * The first-run wizard (A1). It runs once, before anybody has an account, and makes the admin.
   * Port and folders are compose's, so the server step shows them and says where they change.
   */
  setup: {
    title: 'Set up Lapidary',
    step: (n: number, of: number) => `Step ${n} of ${of}`,
    accountTitle: 'Your account',
    accountLead:
      'You will be this install’s admin: you add the other people who use it. Nobody else can see your models unless you share them.',
    codeLabel: 'Setup code',
    codeHint: 'The server printed it when it started. Run `docker compose logs app` and look for “setup code”.',
    usernameLabel: 'Username',
    usernameHint: 'Letters, digits, dots, dashes or underscores. You sign in with it.',
    displayNameLabel: 'Display name',
    passwordLabel: 'Password',
    passwordHint: 'At least 8 characters.',
    confirmLabel: 'Password, again',
    passwordsDiffer: 'The two passwords are different. Type the same one twice.',
    passwordShort: 'A password needs at least 8 characters.',
    missing: 'Fill in the setup code, a username and a password.',
    serverTitle: 'This server',
    serverLead: 'Where Lapidary answers, where your models are, and where it keeps its own files.',
    port: 'Port',
    portHint: 'Where Lapidary answers on this machine. Change it now or later in Settings, with no restart.',
    modelsFolder: 'Your models folder',
    modelsFolderHint: 'Where your models are, and where uploads, imports and removed models go. You can choose another later in Settings.',
    storeHint:
      'Pictures and Lapidary’s copies, in a folder of their own. Choose it now: what is there is copied over, and later it stays put.',
    optionsTitle: 'Options',
    importLinks: 'Import models from Printables, Thingiverse and MyMiniFactory links',
    importLinksHint:
      'Lapidary contacts those sites only when you paste a link. Turn this off for an install that must contact nobody.',
    importLinksForcedOff: 'Turned off by LAPIDARY_IMPORT_LINKS=false in deploy/.env.',
    keysLater: 'Keys for Thingiverse and MyMiniFactory go in Settings once you are signed in.',
    next: 'Next',
    back: 'Back',
    finish: 'Finish setup',
    finishing: 'Setting up…',
    failed: 'Setup did not finish. Check that the server is running, then try again.',
    loading: 'Loading…',
  },
  /** Signing in (A1). */
  login: {
    title: 'Sign in',
    lead: 'Sign in to see your models and the ones shared with you.',
    usernameLabel: 'Username',
    passwordLabel: 'Password',
    submit: 'Sign in',
    signingIn: 'Signing in…',
    failed: 'Could not sign in. Check that the server is running, then try again.',
    forgot: 'Forgot your password? Ask this install’s admin to reset it.',
  },
  /** The signed-in person's profile (M2; per person since A1). */
  profile: {
    title: 'Profile',
    lead: 'What Lapidary calls you, in the account menu and wherever your name is shown.',
    nameLabel: 'Display name',
    passwordHeading: 'Password',
    currentPassword: 'Current password',
    newPassword: 'New password',
    changePassword: 'Change password',
    passwordChanged: 'Changed. Your other sessions are signed out.',
    save: 'Save',
    saved: 'Saved.',
    loading: 'Loading your profile…',
    failed: 'Could not load the profile. Check that the server is running, then reload.',
    saveFailed: 'The name was not saved. Check that the server is running, then try again.',
  },
  /** Importing a model from the page a person pastes (F3). */
  /**
   * The Import dialog (A2): a file browser over the models folder. A person adds a file or a folder,
   * optionally with its page and pictures; nothing is copied, and the library holds what was added.
   */
  addModels: {
    button: 'Import',
    title: 'Import from your models folder',
    lead: 'Choose files or folders. Lapidary reads them where they are; nothing is copied or moved.',
    top: 'Models folder',
    open: (name: string) => `Open ${name}`,
    select: (name: string) => `Add ${name}`,
    empty: 'No folders or model files here.',
    size: (bytes: number) =>
      bytes < 1_048_576
        ? `${Math.max(1, Math.round(bytes / 1024)).toLocaleString('en-US')} KB`
        : `${(bytes / 1_048_576).toLocaleString('en-US', { maximumFractionDigits: 1 })} MB`,
    truncated: 'This folder holds more than the list shows. Open a folder inside it to see the rest.',
    loading: 'Loading…',
    failed: 'Could not read this folder. Check that the server is running, then try again.',
    pageLabel: 'Model page (optional)',
    pageHint: 'Printables, Thingiverse or MyMiniFactory. Gives every model added here its source and licence.',
    fetchDetails: 'Fetch pictures, tags and licence from the page',
    picturesLabel: 'Pictures (optional)',
    picturesChosen: (n: number) => (n === 1 ? '1 picture chosen' : `${n} pictures chosen`),
    add: (n: number) => `Add ${n.toLocaleString('en-US')} selected`,
    addThis: 'Add this whole folder',
    adding: 'Adding…',
    added: (n: number) => (n === 1 ? 'Added. The library is scanning it now.' : `Added ${n}. The library is scanning them now.`),
    inLibrary: 'In this library',
    nothingAdded: 'Nothing added yet.',
    remove: 'Remove',
    removeTitle: (path: string) => `Remove ${path} from this library?`,
    removed: (n: number) =>
      n === 1 ? 'Removed, with its 1 model. Its files stay where they are.' : `Removed, with its ${n} models. Its files stay where they are.`,
    whole: 'The whole models folder',
    otherWays: 'Or:',
    upload: 'Upload files',
    uploadFolder: 'Upload a folder',
    fromLink: 'Import from a model’s page',
  },
  importLink: {
    menuItem: 'Import from a link…',
    title: 'Import from a link',
    lead: 'Paste a model’s page on Printables, Thingiverse or MyMiniFactory. Its STL, OBJ and 3MF files are imported into this library with the page’s name, creator, licence, tags and picture.',
    label: 'Model page address',
    confirm: 'Import',
    failed: 'The import could not be queued. Check that the server is running, then try again.',
    running: 'Importing from the model’s page…',
    finished: (ingested: number, skipped: number, revised: number) =>
      `Import complete — ${ingested.toLocaleString('en-US')} added, ${revised.toLocaleString('en-US')} revised, ${skipped.toLocaleString('en-US')} already here.`,
    unknown: 'That import is not one this library knows about. Import the link again.',
  },
  /** The header every page shares. */
  frame: {
    /** The accessible name of the header's nav — "navigation" alone says nothing. */
    places: 'Places',
    /** The grid of a library's parts, as a place beside Dashboard. */
    parts: 'Parts',
    /** The narrow-screen button that slides the places, and a grid's categories and filters, in over the page. */
    openDrawer: 'Menu',
    closeDrawer: 'Close menu',
  },
  toolbar: {
    view: 'View',
    library: 'Library',
    layout: 'Layout',
    upload: 'Upload',
    uploadFolder: 'Upload a folder',
    /** The Upload menu's two ways in (A10): files, archives among them, or a whole folder. */
    uploadFiles: 'Files…',
    uploadFilesHint: 'stl, obj, 3mf, or a zip or 7z of them',
    uploadAFolder: 'A folder…',
    uploadAFolderHint: 'Its subfolders become categories',
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
   * The dashboard (Phase 6): its widgets, their settings, and moving them about. Goal G5's.
   *
   * Two things every string here has to keep true. **The page costs one request** — so nothing
   * says "refreshing" or offers a reload of one panel, because there is no per-widget endpoint
   * to reload it from; a widget that ran out of time offers a retry, which asks once more.
   * And **the arrangement is this browser's**, not the library's and not the account's: there
   * are no accounts until Phase 8, so the copy says browser wherever it says remembered.
   */
  dashboard: {
    /** The nav's fourth place and the page's headline, one entry so the two cannot drift. */
    title: 'Dashboard',
    lead: 'What your libraries hold, what they occupy, and what the worker is still doing. Everything here arrives in one request, and updates when the server says something changed.',
    /** Said once, under the lead. The honest scope, exactly as the grid's preferences say it. */
    perBrowser: 'This arrangement is remembered by this browser.',
    loading: 'Loading the dashboard…',
    loadFailed:
      'Could not load the dashboard. Check that the api service is running, then try again.',
    /** One dashboard, one retry: there is no per-widget endpoint for a narrower one. */
    retry: 'Try again',
    /** WCAG 2.5.3 — the name a screen reader hears begins with the words on the button. */
    retryLabel: (label: string) => `Try again — ${label}`,
    empty: 'Nothing on this dashboard yet',
    emptyLead:
      'Add a widget for whatever you want in front of you: what a library holds, what it occupies, what the worker is still doing, or the parts added most recently.',

    /** The seven kinds, as the registry names them. A kind added in Rust needs a label here. */
    storageLabel: 'Library storage',
    instanceStorageLabel: 'All storage',
    recentLabel: 'Recently added',
    savedFilterLabel: 'Saved filter',
    /**
     * The kind, in the Add menu. Its *heading* names the facet instead — three of these on one
     * board all read the same otherwise, which is what the 1440 pass found.
     */
    facetLabel: 'Value counts',
    facetFormatLabel: 'Formats',
    facetMaterialLabel: 'Materials',
    facetTagLabel: 'Tags',
    queueLabel: 'Jobs',
    /**
     * Which library a widget is about, in its heading.
     *
     * The name is the one stored when the widget was added, because no widget's value carries
     * its library and two of these panels side by side would otherwise be indistinguishable.
     * A saved filter's heading uses the name the server just sent instead, which is why that
     * one is the only kind whose title can be renamed underneath it and still be right.
     */
    inLibrary: (label: string, library: string) => `${label} — ${library}`,

    /** A widget with nothing to show. Said rather than left blank: blank reads as loading. */
    noParts: 'No parts here yet.',
    noValues: 'Nothing to count yet.',
    /** A facet value's tally, or the word for one past the exact-count threshold. */
    facetTally: (count: number | null) =>
      count === null ? 'present' : count.toLocaleString('en-US'),
    /**
     * The worker's queue for one library. Only the clauses that are true: a standing row of zeroes is
     * noise, and the moment one is not zero it is the only thing on the widget worth reading.
     */
    queueLine: (pending: number, running: number, failed: number) => {
      const clauses: string[] = []
      if (running > 0) clauses.push(`${running.toLocaleString('en-US')} running`)
      if (pending > 0) clauses.push(`${pending.toLocaleString('en-US')} waiting`)
      if (failed > 0) clauses.push(`${failed.toLocaleString('en-US')} failed`)
      return clauses.length === 0 ? 'Nothing queued.' : `${clauses.join(' · ')}.`
    },

    /** A key that ran out of time or failed, while every other key on the page rendered. */
    timedOut: 'This took longer than two seconds to work out.',
    widgetFailed: 'This could not be worked out.',

    /** The grip a widget is dragged by, and the control the arrow keys act on. */
    moveLabel: (label: string) => `Move ${label}`,
    moveHint: 'Drag to move. Arrow keys move it a column or a row; hold Shift to resize it.',
    resizeLabel: (label: string) => `Resize ${label}`,
    /** The live region every move and resize is announced in. */
    positions: 'Widget positions',
    moved: (label: string, group: string, column: number, row: number) =>
      `${label} is now in ${group}, column ${column} of 12, row ${row}.`,
    resized: (label: string, width: number, height: number) =>
      `${label} is now ${width} of 12 columns wide and ${height} rows tall.`,

    /** One widget's own menu: where it goes, what it is configured to show, and removing it. */
    widgetMenu: (label: string) => `Options — ${label}`,
    settings: 'Settings…',
    settingsTitle: (label: string) => `${label} settings`,
    settingsSave: 'Save',
    removeWidget: 'Remove from the dashboard',
    /** Nothing is deleted by removing a widget, and the wording has to keep that clear. */
    removeWidgetDetail: 'Nothing is deleted — this only takes the panel off the page.',
    moveToGroup: (group: string) => `Move to ${group}`,

    /** Named groups: each a titled section with its own grid. */
    defaultGroup: 'Overview',
    groupMenu: (group: string) => `Options — ${group}`,
    addGroup: 'Add a group',
    addGroupTitle: 'Add a group',
    groupNameLabel: 'Group name',
    addGroupConfirm: 'Add the group',
    newGroupName: (number: number) => `Group ${number}`,
    renameGroup: (group: string) => `Rename ${group}`,
    renameGroupTitle: (group: string) => `Rename ${group}`,
    renameGroupConfirm: 'Rename',
    groupUp: (group: string) => `Move ${group} up`,
    groupDown: (group: string) => `Move ${group} down`,
    removeGroup: (group: string) => `Remove ${group}`,
    /** A group is never removed with its widgets inside it; they move to the first group. */
    removeGroupKeeps: (group: string, into: string) =>
      `Remove ${group}? Its widgets move to ${into}. Nothing is deleted.`,
    removeGroupConfirm: 'Remove the group',
    /** The last group cannot go: a widget has to be somewhere. */
    lastGroup: 'A dashboard keeps at least one group.',

    /** Adding a widget: which kind, then the two or three things that kind needs to know. */
    add: 'Add a widget',
    addTitle: 'Add a widget',
    addKind: 'What to show',
    addLibrary: 'Which library',
    addFilter: 'Which saved filter',
    addFacet: 'Count by',
    addLimit: 'How many to show',
    addGroupField: 'In which group',
    addConfirm: 'Add it',
    full: (most: number) =>
      `A dashboard holds ${most} widgets, and this one is full. Remove one to add another.`,
    noFilters: 'This library has no saved filters yet. Save one on the grid first.',
    noLibraries: 'No libraries to show yet.',
  },
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
    /** The part's own picture, which is what a card shows since R1. */
    pictureAlt: (name: string) => `Picture of ${name}`,
    /** A card with several pictures (M5): its arrows, and where in the set the one shown sits. */
    previousPicture: 'Previous picture',
    nextPicture: 'Next picture',
    pictureOf: (index: number, total: number) => `${index} of ${total}`,
    /**
     * A card for a part nobody has given a picture to.
     *
     * **It names the format rather than apologising.** "No preview yet" was true of a render
     * that had not been drawn; nothing renders now, so a picture is something a person adds, and
     * a well that says "not yet" about it is a well that blames us for a decision nobody made.
     * The format is the one useful thing we know about a part with no picture — it says what the
     * file is and what will open it.
     */
    noPicture: (format: string | null) => (format === null ? 'No picture' : format.toUpperCase()),
    /** A pack's card line (A13): what it is and what it weighs. */
    pack: (size: number | null) => (size === null ? 'Pack' : `Pack · ${bytes(size)}`),
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
     * A file in the owner's folders or Lapidary's inbox (M4): read where it is, never compressed or
     * copied, so the only figure is its size and the only fact worth adding is where it is.
     */
    storedWhere: (stored: number, folder: string) => `${bytes(stored)}, in ${folder}`,
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
     * What the probe saved, said once, after the transfer is queued: files this library already
     * indexes at that path with those bytes. There used to be a second half, bytes some other
     * library held; since M4 an upload is its own file in the inbox, so every new file is sent.
     */
    saved: (alreadyHere: number) =>
      alreadyHere > 0 ? `Skipped ${alreadyHere.toLocaleString('en-US')} already here.` : '',
    /** Nothing in the drop was worth sending, which is a success and reads as one. */
    nothingToDo: 'Every file in that folder is already in this library.',
    /**
     * An upload's batch, once its files are committed. Its own copy and not the scan's: an
     * upload has no walk job, so `scan.walking` said "Reading the folder…" for as long as the
     * files were adding, and `scan.finished` called files nobody scanned a scan.
     */
    batchRunning: (done: number, total: number) =>
      `Adding — ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} files.`,
    batchFinished: (ingested: number, skipped: number, revised = 0) =>
      `Upload complete — ${settled(ingested, revised, skipped)}.`,
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
    /** A pack (A13): an archive kept as one item, never unpacked. */
    pack: {
      title: 'Inside this pack',
      count: (n: number) =>
        n === 1
          ? 'One file, read from the archive’s index.'
          : `${n.toLocaleString('en-US')} files, read from the archive’s index.`,
      more: (n: number) => `And ${n.toLocaleString('en-US')} more.`,
      inside: (folder: string) => `All in ${folder}/`,
      size: (value: number) => bytes(value),
    },
    /** The ⋯ button on a part's page: Render, Move, the storage path and Remove. */
    more: 'More actions',
    /** The part page's index of the sections under the studio. */
    sections: 'On this page',
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
    /**
     * The part's own mass, on its page rather than only in its history. Always with the ≈: volume times a density somebody typed is never a measurement, whatever the
     * volume was read from. `massSource` is what says so in words.
     */
    massValue: (grams: number) =>
      grams < 1000
        ? `${grams.toLocaleString('en-US', { maximumFractionDigits: 1 })} g`
        : `${(grams / 1000).toLocaleString('en-US', { maximumFractionDigits: 3 })} kg`,
    /** Under the mass, naming the material the density came from. */
    massSource: (material: string) => `${material}, at the density set for this library`,
    /**
     * Why there is no mass, rather than a blank a reader would take for "weighs nothing". Said in the
     * order the page checks them, which is the order a reader can act in: **no volume first**, because
     * an open mesh told to set a density would still have no mass after setting one. Then a part
     * somebody has said holds none, which must not be told to give it a material — that is the
     * decision it just recorded.
     */
    massNoVolume: 'Not available — without a volume there is nothing to multiply',
    massHoldsNone: 'Not available — this part is recorded as holding no material',
    massNoMaterial: 'Not available — give the part its material, then set that material’s density',
    massSeveralMaterials:
      'Not available — a part of several materials has no one density to work a mass out from',
    massNoDensity: (material: string) =>
      `Not available — no density is set for ${material} in this library`,
    format: 'Format',
    /** The path the part is known by, which since slice 6a is its identity in the
     *  library — two parts called `bracket` in two folders are told apart by this. */
    sourcePath: 'Path',
    revision: 'Revision',
    sourceHash: 'Source hash',
    /** A figure the database has no value for. Distinct from a refusal to measure. */
    unknown: 'Unknown',
    /** A part's revisions, shown once it has more than one (Phase 4 slice 1). */
    history: 'History',
    /**
     * Where a revision's download would be, for one whose file a later revision replaced in the
     * owner's folder (M4): its figures and hash are kept, and its bytes are not anywhere.
     */
    historyNotOnDisk: 'No longer on disk',
    historyNotOnDiskWhy:
      'A later revision replaced this file in your folder, and Lapidary keeps no copy of it. Its figures are still here.',
    historyRevision: (label: string) => `Revision ${label}`,
    /** Where a revision's bytes came from, in the words a person would use for it. */
    origin: { ingest: 'Scanned', upload: 'Uploaded', agent: 'Saved from the desktop app' },
    historyDate: (iso: string) =>
      new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' }),
    historyFailed: "Could not load this part's history. Reload the page to try again.",
    /** Volume times a typed density: never a measurement, so always shown with ≈. */
    mass: 'Mass',
  },
  removal: {
    remove: 'Delete',
    removing: 'Deleting…',
    removeFailed: 'Could not delete this part. Nothing changed — try again.',
    /**
     * Delete is final (owner, 2026-10-17): there is no restore and no removed list. The body
     * says so, and says the one thing that makes it safe to press — the model file itself is
     * never touched, so the part comes back by scanning or uploading that file again.
     */
    confirmTitle: (name: string) => `Delete ${name}?`,
    confirmTitleMany: (count: number) =>
      count === 1 ? 'Delete 1 part?' : `Delete ${count.toLocaleString('en-US')} parts?`,
    confirmBody: (name: string) =>
      `“${name}” is taken out of the library: its pictures, tags and revisions go with it, and this cannot be undone. Its file is moved into _removed in your models folder, never deleted — unless another part still uses it, and then it stays where it is.`,
    confirmBodyMany: (count: number) =>
      `${count === 1 ? 'This part is' : `These ${count.toLocaleString('en-US')} parts are`} taken out of the library: their pictures, tags and revisions go with them, and this cannot be undone. Their files are moved into _removed in your models folder, never deleted — except a file another part still uses, which stays where it is.`,
    confirm: 'Delete',
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
    finished: (ingested: number, skipped: number, revised = 0) =>
      `Scan complete — ${settled(ingested, revised, skipped)}.`,
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
   * Moving a library's source files out of the shared content-addressed store and into
   * each part's own directory. The worker queues this itself on startup for a library
   * upgraded from before folders existed — nothing on this page starts one — so this
   * copy exists to make a batch already running, watched by its id in the URL, read as
   * what it is rather than as a scan of the mounted directory.
   *
   * Its own copy, and not `scan`'s: a migration ingests nothing and
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
    /** Copies `/api/parts/{id}/file`: the current file, whichever revision that is when the link is opened. */
    copyLink: 'Copy link',
    copyLinkFor: (name: string) => `Copy a link to ${name}'s current file`,
    linkCopied: 'Link copied',
    /** A browser on plain http (a LAN install) keeps the clipboard from pages, so the link is shown to copy by hand. */
    linkToCopy: 'Link to this part’s current file',
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
      //
      // "In your folders" since M4: a model's file is read where it is, never copied.
      typeof ratio !== 'number'
        ? `Models ${bytes(source)}, in your folders · files made for other tools ${bytes(derivative)}.`
        : `Models ${bytes(source)}, in your folders · files made for other tools ${bytes(derivative)}, ${(ratio * 100).toLocaleString('en-US', { maximumFractionDigits: 1 })}% of the models.`,
    failed:
      'Could not read what this library occupies. Check that the api service is running, then reload.',

    /**
     * The whole store, across every library, and the figure no per-library line can carry:
     * `quarantined` is on the disk *and* belongs to no library at all, because whatever would
     * have said which one is gone.
     *
     * Conditional: a permanent "0 B on hold" is noise on every installation where nothing is
     * waiting, and the moment some is it is bytes nothing else in the application admits to.
     *
     * **The quarantine clause does not say "purged".** That figure is mostly the thumbnails
     * and rungs *Lapidary* deleted, and telling somebody their store holds deleted data is
     * the data-loss reading `CLAUDE.md` forbids this area from producing. It names the hold
     * and what ends it instead, which is the part a person can act on.
     */
    everything: (source: number, derivative: number, quarantined: number) =>
      `Lapidary’s own store: ${bytes(source + derivative + quarantined)} across all libraries — ${bytes(derivative)} of files made for other tools${source > 0 ? `, ${bytes(source)} of models kept from before Lapidary read your folders where they are` : ''}${quarantined > 0 ? `, ${bytes(quarantined)} on hold for 30 days before the space comes back` : ''}. Your models are in your folders and are not counted here: Lapidary never copies them.`,
    /**
     * The walk is opt-in because it costs the server a look at every file. Worded as the
     * question it answers rather than as the work it does — "measure" is what the user
     * wants; that it is a directory walk is our problem.
     */
    measureOnDisk: 'Measure what is actually on disk',
    measuring: 'Measuring…',
    /**
     * The render cache (Phase 4 slice 2): files Lapidary made and can make again. Worded by the
     * eviction boundary above: "free cache space", never "delete", and no number called freed,
     * because the bytes leave only when their quarantine ends.
     *
     * **Slicer files alone since R3**, and the words had to follow: nothing renders, so there
     * is no "detailed 3D preview" to make again and a sentence promising one would be offering
     * a rebuild nothing could perform. The name of the figure is unchanged because it is on the
     * wire; what it describes is not.
     */
    renderCache: (size: number) =>
      `${bytes(size)} of slicer files nobody has downloaded in 90 days. Lapidary writes each one again when it is next asked for.`,
    freeCache: 'Free cache space…',
    freeCacheTitle: 'Free cache space?',
    freeCacheBody: (size: number) =>
      `This removes the slicer files Lapidary wrote for parts nobody has downloaded in 90 days. No model file is touched, and no picture: every part still shows its own image and still hands you the original file to view or download. A slicer file is written again when it is next asked for. ${bytes(size)} goes into the 30-day quarantine and becomes free space when that ends.`,
    freeCacheConfirm: 'Free cache space',
    freeCacheFailed: 'Could not free cache space. Check that the api service is running, then try again.',
    cacheFreed: (removed: number, size: number) =>
      `Removed ${removed} slicer ${removed === 1 ? 'file' : 'files'} Lapidary can write again. ${bytes(size)} becomes free space when its 30-day quarantine ends.`,
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
     * `tracked` is every figure above, and it is all of them now. It used to exclude the
     * thumbnails, which were in Postgres: folding them in put the tracked total 5.7 MB above a
     * walk of the store on the library this was measured against, which reads as bytes having
     * gone missing rather than as a category error. `0055` deleted those rows, so there is
     * nothing left in the figures that is not a file in the storage folder — and the sentence
     * stops claiming there is.
     *
     * The disk is then legitimately the larger of the two, by the `metadata.json` beside
     * every model (deliberately counted by nothing) plus whatever the owner has put in the
     * folder — which this layout invites them to do.
     */
    onDisk: (disk: number, tracked: number) =>
      disk >= tracked
        ? `On disk: ${bytes(disk)} in Lapidary’s storage folder. That is ${bytes(disk - tracked)} more than the figures above: anything an older version left there, such as a metadata.json beside a model, or that was put there by hand — untracked, but real.`
        : `On disk: ${bytes(disk)} in Lapidary’s storage folder, which is ${bytes(tracked - disk)} less than the figures above. That should not happen — every one of those should be a file. Something has removed files from the store without going through the app.`,
    onDiskFailed:
      'Could not measure Lapidary’s storage folder. The figures above still stand — they come from the database, not from the disk.',
  },
  /**
   * The category tree beside the grid, moving models between categories, and the folder a
   * model is stored in.
   *
   * A category is a location and never an identity (`FolderId`'s own doc says so), and
   * every string here has to keep that true. Moving a model changes where it is filed and
   * where its directory sits on disk. Deleting a category takes what is inside it out of the
   * library and leaves every model file alone. Neither is evicting the derivative cache —
   * different things this product must never let read as each other, which is why the delete copy below says what stays as plainly as it says
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
    body: 'Its models were deleted from the library with it; their files were moved into _removed in your models folder. The rest of these filters still work without it.',
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
     * What a delete actually does, counted — in models AND in subcategories, because the
     * delete takes every descendant folder as well. Naming only the models is how a category
     * holding twelve empty subcategories used to read "No models are inside it." and then
     * take twelve rows off the sidebar. A destructive confirmation names what it affects, and
     * half of what this one affects is the tree itself.
     *
     * `FolderNode.partCount` is subtree-inclusive for the same reason; the subcategory
     * count is the caller's own walk of the tree it already has, excluding the category
     * being deleted, so the two numbers count different things and neither counts twice.
     *
     * Delete is final (owner, 2026-10-17) and there is no restore, so the copy promises none.
     * What it does promise is the thing that stays true: no model file is touched on disk.
     */
    deleteBody: (parts: number, subcategories: number = 0) => {
      const inside =
        parts === 0 && subcategories === 0
          ? 'Nothing is inside it — no models, no subcategories.'
          : `${opensSentence(countedModels(parts))} and ${countedSubcategories(subcategories)} are inside it, counting every level.`
      const subcategoriesGo =
        subcategories === 0 ? '' : subcategories === 1 ? ' and the subcategory under it' : ' and the subcategories under it'
      const action =
        parts === 0
          ? `Deleting takes this category${subcategoriesGo} out of the library`
          : parts === 1
            ? `Deleting takes this category${subcategoriesGo} and that model out of the library, with its pictures and tags`
            : `Deleting takes this category${subcategoriesGo} and those models out of the library, with their pictures and tags`
      return `${inside} ${action}. This cannot be undone. The models' files are moved into _removed in your models folder, never deleted.`
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
      `The category changed while the confirmation was open: ${countedModels(parts)} and ${countedSubcategories(subcategories)} were deleted, not the numbers shown. Their files are still on disk.`,
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
     * Shown under a relative path, once, where a person is looking at exactly the thing it
     * would fix. Names the variable rather than describing it: somebody editing `deploy/.env`
     * needs the string to search for. Which folder the path is inside depends on where the file
     * is (M4): your own folders, Lapidary's inbox, or — for a part from before — its store.
     */
    directoryPartial: (folder: string, variable: string) =>
      `This is the path inside ${folder}. Set ${variable} to an absolute path in deploy/.env and the full path appears here instead.`,
    /** The three folders a file can be in, as `directoryPartial` names them. */
    rootFolder: {
      library: 'your folders',
      inbox: 'Lapidary’s inbox',
      store: 'Lapidary’s storage folder',
    },
    /**
     * `storagePath` is null: a part indexed before Lapidary kept files in folders at all, whose
     * file is in its store under its hash. It still downloads; there is just no folder to show.
     */
    directoryPending:
      'This model’s file is in Lapidary’s store under its hash, from before files were kept in folders, so there is no folder to show. It still downloads.',
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
    renameKeepsDirectory:
      'A category is this library’s own grouping: renaming it changes what you see here and nothing in your folders.',

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
      'Another category here already has a name that differs from this one only in characters a folder name cannot hold. Pick a different name.',
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
    /** A15: the gallery is in the owner's order, and the first picture is the card's. */
    orderHint: 'Drag pictures to order them. The first is the one on the card.',
    onCard: 'On the card',
    moveEarlier: (label: string) => `Move ${label} earlier`,
    moveLater: (label: string) => `Move ${label} later`,
    reorderFailed: 'The new order was not saved. Reload the page and try again.',
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
    fromFile: 'As the file states. A list you set here is kept instead, and is kept even when it is empty.',
    /**
     * A part somebody has said holds no material — a machined blank whose file names the stock it was
     * cut from, say. Its own state, not an empty list: before L3 removing the last material handed the
     * part straight back to its file, which named the material again.
     */
    none: 'Holds no material. Nothing its file states will replace this.',
    /** The undo: hand the part back to whatever its file states, now and at every later read. */
    reset: 'Use what the file states',
    resetting: 'Handing back…',
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
    /** "Fetch details" (A2): fill the part from its model's page, downloading nothing. */
    fetch: 'Fetch details',
    /** A page's description, kept on its source row (A15); long ones open on request. */
    showAll: 'Show all',
    showLess: 'Show less',
    fetchLabel: 'Model page to fetch details from',
    fetchPlaceholder: 'https://www.printables.com/model/…',
    fetching: 'Fetching the page’s details. Its pictures, tags and licence appear here in a moment.',
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
  /**
   * The 3D view — **asked for, never arriving on its own** (R1). Lapidary is a visual library:
   * a card and a part's page show the part's picture, and the view is what somebody presses
   * **view** for, at which point the browser loads the original file itself.
   */
  viewer: {
    /** Keeps the view on screen as the part's picture (F1): what a person sees is what the card shows. */
    keepView: 'Use this view as the picture',
    keepingView: 'Saving the picture…',
    keptView: 'This view is the part’s picture now',
    keepViewFailed: 'The picture was not saved. Try again, or attach a file instead.',
    label: (name: string) => `3D view of ${name}`,
    /** The press that loads the model. Named for what it does, not for the technology. */
    view: 'View in 3D',
    /** While the original file is on its way and being read. */
    loading: 'Loading the model…',
    /** The view's aids (A12). Every figure they show is read from the mesh, so each carries its ≈. */
    tools: 'View tools',
    tool: { grid: 'Grid', size: 'Size', measure: 'Measure' },
    axisX: 'X',
    axisY: 'Y',
    axisZ: 'Z',
    mm: (value: string) => `${value} mm`,
    gridStep: 'Grid squares ',
    size: 'Size ',
    by: ' × ',
    measureFirst: 'Click a point on the model.',
    measureSecond: 'Click a second point. Drag to turn the view.',
    point: (i: number) => `${i === 0 ? 'A' : 'B'}  `,
    distance: 'Distance ',
    delta: 'ΔX, ΔY, ΔZ ',
    /** A model past what a browser can hold (A11). */
    tooBig:
      'This model has more than 50 million triangles, more than a browser can draw. Download it and open it in your own tool.',
    /** Beside the picture where the browser cannot draw 3D, so the missing button is explained. */
    noWebGL: 'This browser cannot draw 3D here, so there is no view. The download is unaffected.',
    failed:
      'This model could not be drawn in the browser. Download it and open it in your own tool — the file itself is unaffected, and large or unusual meshes are the common reason.',
    /**
     * A format with no loader in the browser. STEP parts never offer **view** at all, so this is
     * reached by a bookmark or a format added to the button's list before its loader — and it
     * names the format, because "could not be drawn" without it sends somebody to look at the
     * file rather than at us.
     */
    unviewable: (format: string) =>
      `Lapidary cannot draw ${format.toUpperCase()} in the browser. Download the file and open it in a tool that reads it.`,
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
      'This library is empty. Choose models from your models folder with Import, upload a folder, or drop one anywhere on this page — every model you add appears here.',
    /** Under the first run's Upload: the gesture with no button of its own, said once where it matters. */
    dropHint: 'Or drag a folder from your file manager and drop it anywhere.',
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
   * The tag index: `/tags`, and one tag's own page (P3 — "tags as places").
   *
   * **Its own block, not the `tags` one above.** That block is the part page's tag *editor* — a
   * field, an Add button, a remove label — and belongs to whoever wrote it. Nothing here is read
   * from another block, and nothing here is read by another screen.
   *
   * The word on screen is **model**, not part: these two pages are the browsing half of the
   * application, and a person scanning a library of miniatures and terrain calls them models. The
   * grid and the part page keep saying "part", which is what the data calls them.
   */
  tagIndex: {
    /** The tab, for `/tags`. Subject first, product last, as every other title is. */
    title: 'Tags — Lapidary',
    /**
     * The tab, for one tag's page. The tag first and nothing before it: somebody with six tabs open
     * reads twelve characters, and "Tags — dragon" spends all twelve saying the same word six times.
     */
    tagTitle: (tag: string) => `${tag} — Lapidary`,
    heading: 'Tags',
    /**
     * Under the heading. It says where tags come from, because the answer is "somebody typed it" and
     * a library nobody has tagged is an empty page that would otherwise read as a broken one.
     */
    lead: 'Every tag in this library, and how many models carry it. Tags are typed on a model by hand — nothing reads one off a file.',
    /** The count beside a tag, with its unit, so the link reads whole and needs no label of its own. */
    models: (parts: number) =>
      `${parts.toLocaleString('en-US')} ${parts === 1 ? 'model' : 'models'}`,
    /** The two orders. A group of radios, because they are two views of one list. */
    order: 'Order',
    byCount: 'Most used',
    byName: 'A to Z',
    /** The letter jump, drawn only once the list is long enough to need one. */
    jump: 'Jump to a letter',
    /** The heading a tag starting with something other than a letter is filed under. */
    otherInitial: 'Numbers and symbols',
    /** A library nobody has tagged yet. Not a failure, and it says what to do about it. */
    none: 'No model in this library is tagged yet. Open a model and add a tag to it — a project, a use, a shelf — and it will be here.',
    failed: 'Could not load this library’s tags. Check that the api service is running, then reload.',
    /** The link out of the grid’s filter panel, into the whole library’s tags. */
    all: 'All tags…',
    /** The link back, from one tag’s page. */
    back: 'All tags',

    /** One tag's page: the grid narrowed to it, with what else is on those models beside it. */
    related: 'Often together with',
    /**
     * How the floor is stated. The number is the server's (`RelatedTags.floor`) and not written here
     * twice: a tag sharing one model is a coincidence, and a panel that does not say where the line
     * is invites the reader to guess it.
     */
    relatedNote: (floor: number) =>
      `Tags on at least ${floor.toLocaleString('en-US')} of the same models.`,
    /** How many models a related tag shares. Not that tag's own total, which is why it says "shared". */
    shared: (parts: number) =>
      `${parts.toLocaleString('en-US')} shared`,
    /** Nothing shares enough models to be worth offering. */
    relatedNone: 'Nothing else is tagged on enough of these models to be worth following.',
    relatedFailed: 'Could not load the tags near this one. The models below are unaffected.',
    /**
     * A tag no model carries any more. The honest
     * sentence is that the models went, not that the tag was deleted — there is no tag to delete.
     */
    gone: (tag: string) =>
      `No model in this library carries “${tag}” any more. The last model with it was deleted, or the tag was taken off it.`,
    /**
     * How many models carry the tag. **In this library**, and it says so: the grid under it is the
     * same models narrowed by whatever else is chosen, so "74 models carry this tag" over "Showing
     * all 6 parts" reads as a contradiction rather than as two true counts of different things.
     */
    counted: (parts: number) =>
      parts === 1
        ? '1 model in this library carries this tag'
        : `${parts.toLocaleString('en-US')} models in this library carry this tag`,
  },
} as const
