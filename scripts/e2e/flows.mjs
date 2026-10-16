#!/usr/bin/env node
// The flows: what a person does with this application, each one driven in a real browser against the
// real api, the real worker and the real database.
//
//   node scripts/e2e/flows.mjs --out <run dir> --web <url> --api <url> --seed <seed.json> [--only a,b]
//
// A flow is data — `{ name, route, widths, pending?, run(page, ctx) }` — and the runner does the same
// thing to every one of them: navigate fresh, run it inside its own try/catch, screenshot either way,
// append a row. So a flow cannot leave the next one wedged, a failure still produces the picture that
// explains it, and adding the nineteenth is writing one object.
//
// Two rules the flows keep, because a run has to be repeatable against one `--keep` stack and stage 6
// asks for two runs with identical statuses:
//
//   * a flow that changes anything asserts a **delta**, never an absolute count, and puts back what it
//     took (`delete` deletes only a part it uploaded itself, `saved-filter` deletes its filter, `upload`
//     stamps its bytes so the same file is never sent twice);
//   * every selector a flow depends on is a named entry in `S` below, so the goal's "rename one
//     selector and watch that flow alone fail" test is a one-line edit.
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { session, sleep, uploadThrough } from './cdp.mjs'

const { values: args } = parseArgs({
  options: {
    out: { type: 'string' },
    web: { type: 'string' },
    api: { type: 'string' },
    seed: { type: 'string' },
    only: { type: 'string' },
  },
})
for (const required of ['out', 'web', 'api', 'seed']) {
  if (args[required] === undefined) throw new Error(`--${required} is required`)
}
const seed = JSON.parse(readFileSync(args.seed, 'utf8'))
const only = args.only?.split(',').map((s) => s.trim())

// ---------------------------------------------------------------------------------------------------
// Every selector, in one place.
//
// This application has no `data-testid` anywhere and T1 owns none of `web/`, so every one of these is
// an accessible query against what the app already promises a screen reader: a role, an `aria-label`,
// an `aria-pressed`, an `aria-current`, or one of the fourteen hard-coded ids. Where an id is generated
// by `useId()` the query matches heading text instead, because `«r3»` is not a selector.

const S = {
  // `article` alone is wrong: the open quick look and the whole part page are also `<article>`. Only a
  // card carries `aria-labelledby="part-name-<id>"`.
  card: 'article[aria-labelledby^="part-name-"]',
  // The part's OWN picture. `Rendered preview of …` was the alt text until R1; nothing renders, and
  // `strings.parts.thumbnailAlt` went with sharing (S3).
  cardPicture: 'article[aria-labelledby^="part-name-"] img[alt^="Picture of "]',
  cardCheckbox: 'article[aria-labelledby^="part-name-"] input[type="checkbox"]',
  search: 'input[type="search"][aria-label="Search this library"]',
  // The count line beside the scope heading. No role and no id, so it is scoped to #parts.
  countLine: '#parts p.tabular',
  formatFacet: 'section[aria-labelledby="format-facet"]',
  folderTree: 'nav[aria-label="Categories"]',
  folderCurrent: 'nav[aria-label="Categories"] button[aria-current="true"]',
  viewMenu: 'view-menu',
  libraryMenu: 'library-menu',
  partMenu: 'part-menu',
  drawerToggle: 'button[aria-controls="drawer"]',
  bulkBar: 'section[aria-label="Selected parts"]',
  // At 1440 the quick look is a non-modal <aside>; under 1280 the same children are a modal dialog.
  quickLookPane: 'aside[aria-labelledby]',
  dialog: '[role="dialog"][aria-modal="true"]',
  close: 'button[aria-label="Close"]',
  viewer: 'div[role="img"][aria-label^="3D view of "]',
  // Two hidden file inputs exist; only this one takes models. The other imports a bundle.
  fileInput: 'input[type="file"][webkitdirectory]',
  // Not `#parts p[role="status"]` on its own: the grid page can hold four status regions at once — the
  // grid skeleton, this transfer line, ScanProgress and the review offer — and a bare role query there
  // takes whichever came first in the DOM. The flow matches this line's own vocabulary instead; see
  // `W` below for every wording the rig depends on.
  statusIn: '#parts [role="status"], #parts p[aria-live], #parts div.mb-4.max-w-prose > p',
  historySection: '#part-history',
}

/**
 * The wordings this rig depends on, because the DOM does not identify them.
 *
 * Every entry here is a `strings.ts` value that a flow matches as **text**, and it is a list rather than a
 * scattering of literals so that a copy change breaks one named thing. Three of these are load-bearing in
 * a way a selector normally would be:
 *
 *   `upload.*`  the transfer line has `role="status"`, and so do the grid skeleton, ScanProgress and the
 *               review offer — four on one page. Only the wording tells them apart.
 *   `scan.*`    ScanProgress has no id, deliberately (an id existing only for a driver is scaffolding), so
 *               its text is the only handle there is.
 *   `loadingMesh`  `open-timing.mjs` parses this same literal out of `strings.ts` for the same reason.
 */
// The nested folder the upload flow sends, and asserts its parts are filed under.
const UPLOAD_CATEGORY = 'Brackets'
// [source in the repo, name in the uploaded folder]. Binary STLs only — see the flow for why.
const UPLOAD_FILES = [
  ['fixtures/bracket-lp-1042-03.stl', 'bracket-lp-1042-03.stl'],
  ['example/parts/hex-spacer-m4x20-lp-2145-01.stl', 'hex-spacer-m4x20-lp-2145-01.stl'],
]

/**
 * Copies `files` ([source in the repo, name]) into `dir` with `stamp` in each 80-byte header: identical
 * bytes would be `Skipped`, and a flow that only passes the first time fails the rig's own
 * repeatability test. **Binary** STLs only. The stamp overwrites the first 80 bytes, which is a binary
 * STL's header and an ASCII STL's opening `solid …` line — stamping `fixtures/cube.stl` corrupted it and
 * the app said so, correctly ("1 file could not be read"). `fixtures/spacer-lp-2001-00.stl` and
 * `cube.stl` are the two ASCII ones in the tree; everything under `example/parts/` is binary.
 */
function stampInto(dir, files, stamp, ctx) {
  mkdirSync(dir, { recursive: true })
  for (const [from, name] of files) {
    const bytes = readFileSync(join(ctx.root, from))
    Buffer.from(stamp.slice(0, 79).padEnd(80, '\0'), 'binary').copy(bytes, 0)
    writeFileSync(join(dir, name), bytes)
  }
}

/** The header's Upload button, pressed for real, because Chrome opens no file chooser without user activation. */
function pressUpload(page, folder) {
  return uploadThrough(
    page,
    async (p) => {
      const at = await p.evaluate(`(() => {
        const b = [...document.querySelectorAll('header button')].find((e) => e.textContent.includes(${JSON.stringify(T.uploadButton)}))
        if (!b) return null
        const r = b.getBoundingClientRect()
        return { x: r.x + r.width / 2, y: r.y + r.height / 2 }
      })()`)
      expect(at !== null, `no header button reads "${T.uploadButton}"`)
      await p.click(at.x, at.y)
    },
    [folder],
    { mode: 'selectMultiple' },
  )
}

const W = {
  upload: ['Reading ', 'Asking which files are new', 'Uploading ', 'Finishing the upload'],
  scanRunning: /Reading the folder|Scanning — [\d,]+ of/,
  scanDone: /Scan complete — [^.]*\./,
}

// Text the flows match on: the rendered strings, because that is what the person reads. Every one of
// these is a `strings.ts` value, and a rename there should fail exactly the flow that depends on it.
const T = {
  loadMore: 'Load more',
  select: 'Select',
  order: 'Order',
  cardsPerPage: 'Cards per page',
  saveThisFilter: 'Save this filter',
  filterNameLabel: 'Name for this filter',
  // The part menu's row and the confirmation's button carry the same word.
  deletePart: 'Delete',
  // The sentence the delete confirmation exists to say: delete is final, and the file is not touched.
  deleteKeepsFile: 'The file itself stays where it is on disk.',
  uploadButton: 'Upload a folder',
  scanStart: 'Scan the ingest folder',
  storageSummary: 'Storage',
  uploadComplete: 'Upload complete —',
  // The press that loads the model, and the whole of `view-original`. `open-timing.mjs` parses this
  // same literal out of strings.ts for the same reason.
  viewButton: 'View in 3D',
  // The provenance title on a figure read from a mesh. **"Figures must not lie": this is the mark**,
  // and the `≈` beside it is aria-hidden, so the mark is what an assertion must read.
  //
  // There is no opposite any more. `Read from an analytic CAD entity` is still in `strings.ts`, for
  // rows a STEP file wrote before the pivot, but nothing this rig ingests can carry it — a B-rep file
  // is refused at ingest now, so every figure in a library built by this rig comes from a mesh and
  // every one of them must say so.
  approximateTitle: 'Derived from tessellated',
}

// ---------------------------------------------------------------------------------------------------
// Assertion helpers. A flow fails by throwing; it succeeds by returning what it measured, so the
// report carries a number and not just a green tick.

const fail = (message) => {
  throw new Error(message)
}
const expect = (condition, message) => {
  if (!condition) fail(message)
  return true
}
const gridReady = `document.querySelectorAll(${JSON.stringify(S.card)}).length > 0`
const cards = (page) => page.evaluate(`document.querySelectorAll(${JSON.stringify(S.card)}).length`)
const cardsSettled = (page, accept = (n) => n > 0) =>
  page.settle(() => cards(page), accept, { timeout: 25_000 })
const names = (page) =>
  page.evaluate(
    `[...document.querySelectorAll(${JSON.stringify(S.card)})].map((a) => (document.getElementById(a.getAttribute('aria-labelledby'))?.textContent ?? '').trim())`,
  )
const exists = (page, selector) =>
  page.evaluate(`document.querySelector(${JSON.stringify(selector)}) !== null`)
const text = (page, selector) =>
  page.evaluate(`document.querySelector(${JSON.stringify(selector)})?.textContent?.trim() ?? null`)
const param = (page, key) =>
  page.evaluate(`new URL(location.href).searchParams.get(${JSON.stringify(key)})`)

/** The grid of one library. Every grid flow starts here, so none inherits another's filter. */
const grid = (library, query = '') => `/?library=${library}${query}`

/**
 * Open the rail.
 *
 * Below 768px the saved filters, the three facets, the category tree and the storage panel all live in
 * `#drawer`, which is `visibility: hidden` and translated off-screen rather than unmounted — so
 * `querySelector` finds them and a click never lands. Above it the toggle is `display: none`, so this
 * is a no-op there, which is why every rail flow can call it unconditionally.
 */
async function openRail(page) {
  if (page.width >= 768) return false
  const opened = await page.evaluate(`(() => {
    const t = document.querySelector(${JSON.stringify(S.drawerToggle)})
    if (!t || t.getAttribute('aria-expanded') === 'true') return false
    t.click()
    return true
  })()`)
  if (opened) await sleep(400)
  return opened
}

/** One `<select>` in the View menu, found by its wrapping label's text — neither has an id. */
const viewSelect = (label) =>
  `(() => {
    const l = [...document.querySelectorAll('#view-menu label')].find((e) => e.textContent.startsWith(${JSON.stringify(label)}))
    return l ? l.querySelector('select') : null
  })()`

/**
 * Every request the page makes from now on, as a list the caller reads after the fact.
 *
 * `cdp.mjs`'s `collect` already does the hard part; this names the two questions the pivot made worth
 * asking. **Not the extension's network log** — T1 recorded that it misreports some statuses — but
 * `Network.requestWillBeSent`, which is the browser saying what it asked for, and the only thing
 * asserted here is the URL.
 */
async function watchRequests(page) {
  await page.send('Network.enable')
  const seen = page.collect('Network.requestWillBeSent')
  const from = seen.length
  return {
    urls: () => seen.slice(from).map((e) => e.request.url),
    since: () => seen.length - from,
  }
}

/** Wait until the viewer says it has drawn — the same mark `open-timing.mjs` measures to. */
const firstFrame = (page, timeout = 30_000) =>
  page.settle(
    () => page.evaluate(`performance.getEntriesByName('lapidary:viewer-first-frame').length`),
    (n) => n > 0,
    { timeout },
  )

// ---------------------------------------------------------------------------------------------------
// The flows, in priority order.

const FLOWS = [
  {
    name: 'grid-loads',
    widths: [1440, 390],
    async run(page, ctx) {
      await page.go(grid(ctx.sweep.id), gridReady)
      const n = await cardsSettled(page, (v) => v >= 50)
      expect(n === 50, `the default page size is 50 and the grid rendered ${n} cards`)
      const shown = await names(page)
      const nameless = shown.filter((s) => s.length === 0)
      expect(nameless.length === 0, `${nameless.length} cards rendered with no name`)
      // `N parts so far.` while another page exists; `Showing all N parts.` when they are all in.
      const count = await text(page, S.countLine)
      expect(count !== null, `no count line matched ${S.countLine}`)
      expect(/\bparts?\b/.test(count), `the count line reads ${JSON.stringify(count)}`)
      return `${n} cards, ${JSON.stringify(count)}, first ${JSON.stringify(shown[0])}`
    },
  },
  {
    // **The card is a picture, and the page of them costs one request.**
    //
    // This replaces `thumbnails`, which asserted that every card carried a `data:` render. Nothing
    // renders. What a card shows now is the part's own `part_image` — or, for a part nobody has given
    // one to, its FORMAT as text, which is every card in a real library because nothing produces a
    // picture. Both halves are asserted, and the half worth keeping from the old flow is the second
    // one: **nothing fetches per card.**
    name: 'pictures',
    widths: [1440, 390],
    async run(page, ctx) {
      // The seeded picture first, on the one part in this tree we have a real photograph of. Reached by
      // search rather than by position: which cards land on page one depends on the default order, and
      // a flow that assumed it would start passing and failing with a sort change.
      await page.go(grid(ctx.sweep.id, '&q=bracket'), gridReady)
      await page.waitFor(`new URL(location.href).searchParams.get('q') === 'bracket'`, 10_000)
      const pictured = await page.settle(
        () =>
          page.evaluate(`(() => {
            const img = document.querySelector(${JSON.stringify(S.cardPicture)})
            return img === null ? null : { alt: img.alt, inline: img.src.startsWith('data:'), bytes: img.src.length }
          })()`),
        (r) => r !== null,
        { timeout: 20_000 },
      )
      expect(pictured !== null, `no card matched ${S.cardPicture}; the seed attaches one to the bracket`)
      expect(
        pictured.alt.includes('bracket-lp-1042-03'),
        `the pictured card is ${JSON.stringify(pictured.alt)}, not the bracket the seed photographed`,
      )
      // Inline, because the seeded WebP is under `images.rs`'s 64 KB `INLINE_LIMIT` and the grid query
      // carries those bytes on the row. A larger one would be a hash and one immutable `<img>` load —
      // still not an API request, which is the property below.
      expect(pictured.inline, `the bracket's picture is not inline: ${pictured.bytes} chars of src`)

      // Now the whole grid, and the one-request property.
      const net = await watchRequests(page)
      await page.go(grid(ctx.sweep.id), gridReady)
      const n = await cardsSettled(page, (v) => v >= 50)
      // `content-visibility: auto` on every <li>, so an off-screen card is not laid out. Counting what
      // is in the DOM is therefore the right question and `checkVisibility` is not.
      const got = await page.evaluate(`(() => {
        const all = [...document.querySelectorAll(${JSON.stringify(S.card)})]
        const shown = (a) => {
          const img = a.querySelector('img[alt^="Picture of "]')
          if (img) return 'picture'
          // The placeholder names the format -- strings.parts.noPicture upper-cases it -- and says
          // "No picture" only for a part whose format we do not know. Either is an answer; nothing
          // is not. No backticks in here: one inside a // comment inside a template literal ends the
          // literal, which is T1's own finding and cost this edit a node --check.
          const text = (a.querySelector('div > span')?.textContent ?? '').trim()
          return /^[A-Z0-9]{2,4}$|^No picture$/.test(text) ? 'format' : 'nothing'
        }
        const kinds = all.map(shown)
        return {
          cards: all.length,
          picture: kinds.filter((k) => k === 'picture').length,
          format: kinds.filter((k) => k === 'format').length,
          nothing: kinds.filter((k) => k === 'nothing').length,
        }
      })()`)
      expect(got.cards === n, `${n} cards settled but ${got.cards} were read`)
      expect(
        got.nothing === 0,
        `${got.nothing} of ${got.cards} cards show neither a picture nor their format — an empty well is the one thing a card may not be`,
      )

      // **Nothing fetches per card** — and R1's rule is about an API request, not about every byte.
      //
      // Per-part DATA would have to be addressed by a part id or a revision id, so those are what this
      // forbids outright. A **blob** is deliberately not in that list: an image over `INLINE_LIMIT`
      // (64 KB) arrives on the grid row as a hash and the card emits `/api/blob/{hash}`, which is one
      // immutable `<img>` load and never a second API request. Forbidding it would fail against
      // correct behaviour the first time somebody attaches a large picture. So blob requests are
      // *bounded by the number of cards showing a picture* instead, which is the real rule: at most
      // one per pictured card, and none at all for a card showing its format.
      const urls = net.urls()
      const perPart = urls.filter((u) => /\/api\/(parts|revisions)\//.test(u))
      expect(
        perPart.length === 0,
        `${perPart.length} of ${urls.length} requests address a part or a revision, so something fetched per card: ${JSON.stringify(perPart.slice(0, 3))}`,
      )
      const blobs = urls.filter((u) => u.includes('/api/blob/'))
      expect(
        blobs.length <= got.picture,
        `${blobs.length} blob loads for ${got.picture} card(s) showing a picture — a card without one must load nothing: ${JSON.stringify(blobs.slice(0, 3))}`,
      )
      const api = urls.filter((u) => u.includes('/api/') && !u.includes('/api/blob/'))
      expect(
        api.length < got.cards,
        `${api.length} api requests for ${got.cards} cards — that is per-card, whatever it is addressed by: ${JSON.stringify(api.slice(0, 4))}`,
      )
      return `${got.picture} picture(s) + ${got.format} format placeholder(s) = ${got.cards} cards, none empty; ${api.length} api request(s) for the page and ${blobs.length} blob load(s), none naming a part or a revision`
    },
  },
  {
    name: 'search',
    async run(page, ctx) {
      await page.go(grid(ctx.sweep.id), gridReady)
      const before = await cardsSettled(page)
      const box = await page.rect(S.search)
      expect(box !== null, `no search box matched ${S.search}`)
      await page.click(box.x, box.y)
      await page.type('flange')
      // Debounced 250 ms into the URL, and a single character is swallowed: two is the minimum. The
      // URL is the signal, not a sleep.
      await page.waitFor(`new URL(location.href).searchParams.get('q') === 'flange'`, 10_000)
      const after = await page.settle(
        () => names(page),
        (found) => found.length > 0 && found.every((n) => /flange/i.test(n)),
        { timeout: 20_000 },
      )
      expect(after.length > 0, 'searching for "flange" found nothing; alike/ holds the flange and its byte-copy')
      const stray = after.filter((n) => !/flange/i.test(n))
      expect(stray.length === 0, `results that do not match: ${JSON.stringify(stray.slice(0, 3))}`)
      expect(after.length < before, `search did not narrow: ${before} before, ${after.length} after`)
      // Search forces the order to relevance and says so rather than leaving a stale sort on.
      const note = await text(page, '#sort-while-searching')
      expect(note !== null, 'searching did not explain that the order is now relevance')
      return `${before} -> ${after.length} cards, all matching; ${JSON.stringify(note)}`
    },
  },
  {
    name: 'facet-format',
    async run(page, ctx) {
      const facets = await ctx.get(`/api/libraries/${ctx.sweep.id}/facets`)
      const formats = facets.formats.map((f) => f.value)
      // Three formats, and the facet filtered to STEP is the flow this one used to be: there were six
      // STEP fixtures in the tree and five of them showed. They are still in the tree and they are
      // **refused by name** now, so the right assertion flips — a B-rep format appearing in this facet
      // would mean one of them ingested anyway, which is the thing R2's refusal exists to stop.
      for (const want of ['stl', 'obj', '3mf']) {
        expect(formats.includes(want), `the format facet has no \`${want}\`: ${formats.join(', ')}`)
      }
      for (const banned of ['step', 'stp', 'iges', 'igs']) {
        expect(
          !formats.includes(banned),
          `the facet offers \`${banned}\`, so a B-rep file ingested instead of being refused: ${formats.join(', ')}`,
        )
      }
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      await openRail(page)
      // A <button aria-pressed>, not a checkbox, and its accessible name is `3MF, 1 part` — so the
      // visible inner <span> is what identifies it. 3MF because it is the smallest facet in the tree
      // (one fixture), so a filter that did nothing at all could not pass this.
      const pressed = await page.evaluate(`(() => {
        const section = document.querySelector(${JSON.stringify(S.formatFacet)})
        if (!section) return 'no format facet section'
        const b = [...section.querySelectorAll('button')].find((e) => e.querySelector('span')?.textContent === '3MF')
        if (!b) return 'no 3MF button'
        b.click()
        return b.getAttribute('aria-label')
      })()`)
      expect(!pressed.startsWith('no '), `${pressed} in ${S.formatFacet}`)
      await page.waitFor(`new URL(location.href).searchParams.get('format') === '3mf'`, 10_000)
      const n = await cardsSettled(page, (v) => v > 0 && v < 50)
      const tmf = facets.formats.find((f) => f.value === '3mf')
      expect(n > 0, 'filtering to 3MF showed no cards')
      // `count` is null past the server's exact-count threshold, and the UI then shows no number at all.
      // One 3MF fixture is far under it, so an exact match is the real assertion — but a null must not
      // fail against correct behaviour.
      if (tmf.count === null) {
        expect(n <= 50, `${n} cards on one page of 50`)
      } else {
        expect(n === tmf.count, `${n} cards shown, but the facet counts ${tmf.count} 3MF parts`)
      }
      return `${formats.length} formats (${formats.join(', ')}), no B-rep among them; 3MF ${JSON.stringify(pressed)} shows ${n}`
    },
  },
  {
    name: 'folder-tree',
    async run(page, ctx) {
      const tree = await ctx.get(`/api/libraries/${ctx.sweep.id}/folders`)
      const count = (nodes) => nodes.reduce((sum, n) => sum + 1 + count(n.children ?? []), 0)
      const total = count(tree)
      expect(total >= 8, `the ingest tree should make at least 8 categories; the api reports ${total}`)
      await page.go(grid(ctx.sweep.id), gridReady)
      const all = await cardsSettled(page)
      await openRail(page)
      expect(await exists(page, S.folderTree), `no category tree matched ${S.folderTree}`)
      // The root row, `All models`, is itself a FolderButton and carries aria-current on a bare grid.
      const root = await text(page, S.folderCurrent)
      expect(root !== null, 'no category is marked current, not even the root')
      // A plain <button> whose text is the name — there is no treeitem role and no aria-selected.
      // Named from the API's own tree, not inferred from the markup: a filter on "a button with no
      // aria-label" also matches `New category`, which is what the first real run clicked — and clicking
      // it selects nothing, so the flow failed waiting for a `folderId` that was never going to arrive.
      const wanted = []
      const walk = (nodes) => nodes.forEach((n) => { wanted.push(n.name); walk(n.children ?? []) })
      walk(tree)
      const picked = await page.evaluate(`(() => {
        const names = ${JSON.stringify(wanted)}
        const tree = document.querySelector(${JSON.stringify(S.folderTree)})
        const b = [...tree.querySelectorAll('button')].find((e) =>
          e.getAttribute('aria-current') === null &&
          e.getAttribute('aria-expanded') === null &&
          names.includes(e.textContent.trim()))
        if (!b) return null
        const label = b.textContent.trim()
        b.click()
        return label
      })()`)
      expect(
        picked !== null,
        `no button in the tree carries any of the ${wanted.length} category names the api reported`,
      )
      await page.waitFor(`new URL(location.href).searchParams.get('folderId') !== null`, 10_000)
      // Subtree-inclusive: a category shows what is in it and everything under it.
      const narrowed = await page.settle(() => cards(page), (n) => n !== all, { timeout: 20_000 })
      expect(narrowed !== all, `selecting ${JSON.stringify(picked)} did not change the grid (${all} cards)`)
      const now = await text(page, S.folderCurrent)
      expect(now === picked, `${JSON.stringify(picked)} was clicked but ${JSON.stringify(now)} is current`)
      return `${total} categories; root ${JSON.stringify(root)}; ${JSON.stringify(picked)} narrows ${all} -> ${narrowed}`
    },
  },
  {
    name: 'sort',
    async run(page, ctx) {
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page, (n) => n >= 50)
      // The order lives in localStorage and a react-query key, never in the URL — so the assertion is
      // on the card order, which is the thing the person actually sees change.
      const options = await page.evaluate(`(() => {
        const s = ${viewSelect(T.order)}
        return s ? { value: s.value, options: [...s.options].map((o) => ({ value: o.value, text: o.textContent })) } : null
      })()`)
      expect(options !== null, `no order <select> under a #view-menu label starting "${T.order}"`)
      expect(options.options.length >= 5, `the order control offers ${options.options.length} options`)
      const pick = options.options.find((o) => o.value !== options.value)
      // Read BEFORE the change is dispatched. Reading it after put the grid mid-refetch, `before` came
      // back empty, and `moved` counted 50 differences against nothing — so the flow passed while testing
      // nothing at all. A false pass is the one outcome this rig may not produce.
      const before = await names(page)
      expect(before.length >= 50, `only ${before.length} cards to reorder`)
      const set = await page.evaluate(`(() => {
        const s = ${viewSelect(T.order)}
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(pick.value)})
        s.dispatchEvent(new Event('change', { bubbles: true }))
        return s.value
      })()`)
      expect(set === pick.value, `setting the order to ${pick.value} left it at ${set}`)
      // The whole order, not just the first card. Two orders can legitimately agree on their first part —
      // the fixture-plate assembly is both the newest and the largest by volume — and asserting on
      // `[0]` called that a failure on the first real run. What sorting means is that the sequence moved.
      const reordered = await page.settle(
        () => names(page),
        (list) => list.length >= before.length && list.join('\u0000') !== before.join('\u0000'),
        { timeout: 20_000 },
      )
      const moved = reordered.filter((n, i) => before[i] !== n).length
      expect(
        moved > 0,
        `ordering by ${JSON.stringify(pick.text)} left all ${before.length} cards in the same positions`,
      )
      return `${options.options.length} orders, was ${options.value}; ${JSON.stringify(pick.text)} moves ${moved} of ${before.length} cards, ${JSON.stringify(reordered[0])} first`
    },
  },
  {
    name: 'page-size',
    async run(page, ctx) {
      await page.go(grid(ctx.sweep.id), gridReady)
      const fifty = await cardsSettled(page, (n) => n >= 50)
      expect(fifty === 50, `the default is 50 per page; the grid shows ${fifty}`)
      const set = await page.evaluate(`(() => {
        const s = ${viewSelect(T.cardsPerPage)}
        if (!s) return null
        const has = [...s.options].map((o) => o.value)
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, '250')
        s.dispatchEvent(new Event('change', { bubbles: true }))
        return { value: s.value, has }
      })()`)
      expect(set !== null, `no page-size <select> under a #view-menu label starting "${T.cardsPerPage}"`)
      expect(
        ['50', '100', '250', '500'].every((v) => set.has.includes(v)),
        `the four page sizes are 50/100/250/500; the control offers ${set.has.join(', ')}`,
      )
      // Changing it changes the query key, so the grid resets to page one at the new size.
      const larger = await page.settle(() => cards(page), (n) => n === 250, { timeout: 40_000 })
      expect(larger === 250, `asking for 250 a page gave ${larger} cards`)
      return `50 -> ${larger} cards; sizes ${set.has.join('/')}`
    },
  },
  {
    name: 'paging',
    async run(page, ctx) {
      // There is no next/previous. Paging is an IntersectionObserver sentinel plus one `Load more`
      // button that only exists while another page remains — so the button is the deterministic half
      // and the sentinel is the half that needs the tab frontmost (cdp.mjs calls bringToFront).
      await page.go(grid(ctx.sweep.id), gridReady)
      const first = await cardsSettled(page, (n) => n >= 50)
      const firstNames = await names(page)
      const soFar = await text(page, S.countLine)
      expect(/so far/.test(soFar ?? ''), `with more pages to come the count should say "so far"; it says ${JSON.stringify(soFar)}`)
      const clicked = await page.evaluate(`(() => {
        const b = [...document.querySelectorAll('#parts button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.loadMore)})
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(clicked, `no "${T.loadMore}" button in #parts, and ${first} cards is not the whole library`)
      const grew = await page.settle(() => cards(page), (n) => n > first, { timeout: 30_000 })
      expect(grew > first, `"${T.loadMore}" loaded nothing beyond ${first} cards`)
      // Keyset paged on the last id, so no part can be skipped or repeated by one arriving mid-scroll.
      const all = await names(page)
      const repeats = firstNames.filter((n) => all.filter((m) => m === n).length > firstNames.filter((m) => m === n).length)
      expect(repeats.length === 0, `the second page repeated the first: ${JSON.stringify(repeats.slice(0, 3))}`)
      return `${first} -> ${grew} cards on "${T.loadMore}", no repeats; was ${JSON.stringify(soFar)}`
    },
  },
  {
    name: 'saved-filter',
    async run(page, ctx) {
      // The whole section is absent with nothing saved and no filter set, so the flow filters first —
      // which is also the only order a person can do it in.
      const name = `e2e ${ctx.stamp}`
      // **3MF, not STEP.** This filtered to `format=step` and waited for a card, which with six STEP
      // fixtures was five cards and is now none at all — they are refused at ingest. A filter with no
      // results is a legitimate thing to save, but it is not what this flow is for: it needs the grid
      // to be showing the filter it is about to save, and `aria-current` to say so.
      await page.go(grid(ctx.sweep.id, '&format=3mf'), gridReady)
      await cardsSettled(page)
      await openRail(page)
      const opened = await page.evaluate(`(() => {
        const b = [...document.querySelectorAll('button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.saveThisFilter)})
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(opened, `filtering to 3MF showed no "${T.saveThisFilter}" button`)
      await sleep(300)
      const box = await page.evaluate(`(() => {
        const l = [...document.querySelectorAll('label')].find((e) => e.textContent.startsWith(${JSON.stringify(T.filterNameLabel)}))
        const i = l?.querySelector('input')
        if (!i) return null
        i.focus()
        const r = i.getBoundingClientRect()
        return { x: r.x + r.width / 2, y: r.y + r.height / 2 }
      })()`)
      expect(box !== null, `the save form has no input labelled "${T.filterNameLabel}"`)
      await page.click(box.x, box.y)
      await page.type(name)
      // Scoped to this form: `button[type=submit]` unqualified takes the first one in the document,
      // which on a page with a search form and a pair form is not necessarily this one.
      const submitted = await page.evaluate(`(() => {
        const l = [...document.querySelectorAll('label')].find((e) => e.textContent.startsWith(${JSON.stringify(T.filterNameLabel)}))
        const b = l?.closest('form')?.querySelector('button[type="submit"]')
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(submitted, 'the save form has no submit button of its own')
      // It comes back from the server, not from component state: the list is refetched.
      const saved = await page.settle(
        () => ctx.get(`/api/libraries/${ctx.sweep.id}/filters`),
        (list) => list.some((f) => f.name === name),
        { timeout: 20_000, every: 500 },
      )
      const mine = saved.find((f) => f.name === name)
      expect(mine !== undefined, `the filter was not saved: ${JSON.stringify(saved.map((f) => f.name))}`)
      expect(mine.search.format === '3mf', `it saved ${JSON.stringify(mine.search)} rather than format=3mf`)
      // aria-current marks the filter the grid currently matches, which is the only visible proof the
      // saved thing and the shown thing are the same thing.
      const current = await page.settle(
        () =>
          page.evaluate(`(() => {
            const h = [...document.querySelectorAll('h2')].find((e) => e.textContent === 'Saved filters')
            const s = h?.closest('section')
            return s ? [...s.querySelectorAll('button[aria-current="true"]')].map((b) => b.textContent.trim()) : []
          })()`),
        (rows) => rows.some((r) => r.includes(name)),
        { timeout: 15_000 },
      )
      expect(
        current.some((r) => r.includes(name)),
        `the saved filter is not marked current: ${JSON.stringify(current)}`,
      )
      // Put it back: a run must not leave the next one a longer list.
      await ctx.del(`/api/libraries/${ctx.sweep.id}/filters/${mine.id}`)
      return `saved ${JSON.stringify(name)} as format=3mf, marked current, then removed`
    },
  },
  {
    name: 'selection-bulk',
    async run(page, ctx) {
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      expect((await exists(page, S.bulkBar)) === false, 'the bulk bar is present before anything is selected')
      expect((await exists(page, S.cardCheckbox)) === false, 'cards have checkboxes with selection off')
      // Selection is off by default and a toggle turns it on; until then a click on a card opens it.
      const on = await page.evaluate(`(() => {
        const b = [...document.querySelectorAll('button[aria-pressed]')].find((e) => e.textContent.trim() === ${JSON.stringify(T.select)})
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(on, `no "${T.select}" toggle among the aria-pressed buttons`)
      await page.waitFor(`document.querySelector(${JSON.stringify(S.cardCheckbox)}) !== null`, 8000)
      for (const index of [0, 2]) {
        const at = await page.rect(S.cardCheckbox, index)
        expect(at !== null, `card ${index} grew no checkbox`)
        await page.click(at.x, at.y)
        await sleep(200)
      }
      const selected = await page.settle(
        () => page.evaluate(`document.querySelectorAll('${S.cardCheckbox}:checked').length`),
        (n) => n === 2,
        { timeout: 8000 },
      )
      expect(selected === 2, `ticking two checkboxes selected ${selected}`)
      expect(await exists(page, S.bulkBar), `two parts are selected and no bar matched ${S.bulkBar}`)
      const bar = await page.evaluate(`(() => {
        const s = document.querySelector(${JSON.stringify(S.bulkBar)})
        return {
          live: s.querySelector('[aria-live="polite"]')?.textContent.trim(),
          actions: [...s.querySelectorAll('button')].map((b) => b.textContent.trim()),
        }
      })()`)
      expect(/2 parts selected/.test(bar.live ?? ''), `the bar says ${JSON.stringify(bar.live)}`)
      // Nothing is pressed: every action here changes data, and this flow only proves they are offered.
      return `${JSON.stringify(bar.live)}; actions ${bar.actions.join(', ')}`
    },
  },
  {
    name: 'quick-look',
    widths: [1440, 390],
    async run(page, ctx) {
      const net = await watchRequests(page)
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      // The card's WELL, not its picture: most parts have none, so there is no <img> to aim at. 30 % of
      // the way down lands in the well at every density and layout, and never on the footer, where the
      // name is a link the card's own click filter hands over to.
      const card = await page.rect(S.card, 0)
      expect(card !== null, `no card matched ${S.card}`)
      await page.click(card.left + card.width / 2, card.top + card.height * 0.3)
      // Two shapes by width, and the 1280px boundary is a matchMedia the device-metrics override
      // flips: a non-modal <aside> pane when there is room beside the grid, a modal dialog when not.
      const wide = page.width >= 1280
      const surface = wide ? S.quickLookPane : S.dialog
      await page.waitFor(`document.querySelector(${JSON.stringify(surface)}) !== null`, 20_000)
      await page.waitFor(`new URL(location.href).searchParams.get('part') !== null`, 8000)
      // **It must NOT have drawn, and that is the assertion now.** This flow used to wait for a first
      // frame here, because opening a part loaded its L0 and painted it. Since R1 the stage shows the
      // part's picture and offers **view**; a frame arriving unasked would mean something still loads a
      // mesh because a panel opened. `view-original` is where the press and the frame are tested.
      await sleep(1500)
      const drew = await page.evaluate(`performance.getEntriesByName('lapidary:viewer-first-frame').length`)
      expect(drew === 0, `the quick look drew ${drew} frame(s) without anybody pressing view`)
      const meshes = net.urls().filter((u) => /variant=original|\.glb(\?|$)/.test(u))
      expect(
        meshes.length === 0,
        `opening the quick look fetched a model file: ${JSON.stringify(meshes.slice(0, 2))}`,
      )
      // Not a dialog when it is a pane: it has no aria-modal, the grid stays clickable, and claiming
      // otherwise would be claiming a focus trap this deliberately does not have.
      const modal = await exists(page, S.dialog)
      expect(modal === !wide, `at ${page.width}px the quick look ${modal ? 'is' : 'is not'} modal, which is wrong`)
      await page.mouse(page.width / 2, page.height - 5)
      await sleep(300)
      const close = await page.rect(S.close)
      expect(close !== null, `the quick look has no close control matching ${S.close}`)
      await page.click(close.x, close.y)
      await page.waitFor(`document.querySelector(${JSON.stringify(surface)}) === null`, 8000)
      const part = await param(page, 'part')
      expect(part === null, `closing left ?part=${part} in the url`)
      return `${wide ? 'pane' : 'modal dialog'} at ${page.width}px, nothing drawn and no mesh fetched, closed and ?part cleared`
    },
  },
  {
    name: 'part-detail',
    widths: [1440, 390],
    async run(page, ctx) {
      // **A mesh, repointed from the PMI cylinder.** This flow used to open a STEP fixture and assert
      // that its Volume figure read *Read from an analytic CAD entity* — the analytic half of "figures
      // must not lie". A STEP file is refused at ingest now, so there is no such part to open and no
      // such figure to read: every figure in this library comes from a mesh, which makes the rule
      // simpler rather than weaker. The flange is the one asserted because it is also the part
      // `view-original` draws, so a page and a view are tested on one known solid.
      const part = await ctx.partLike(/alike\/flange-dn40-lp-3310-02\.stl$/)
      const net = await watchRequests(page)
      await page.go(`/parts/${part.id}`, `document.querySelector('h1, h2') !== null`)
      // The section index is built by a MutationObserver over the sections that actually rendered, so
      // it is the page's own account of itself.
      const page_ = await page.evaluate(`(() => {
        const h3 = [...document.querySelectorAll('h3')].map((h) => h.textContent.trim())
        return {
          h3,
          index: [...document.querySelectorAll('nav[aria-label="On this page"] a')].map((a) => a.textContent.trim()),
          file: document.querySelector('#part-file') !== null,
          identity: document.querySelector('#part-identity') !== null,
          // Gone with shape similarity (R3): the section, its heading and every route behind it. Read
          // rather than assumed, because a page that still mounted it would be reading a dropped table.
          likeness: document.querySelector('#part-likeness') !== null,
          // Gone with the kernel: the entity list these two drew from needed a B-rep read.
          specified: document.querySelector('#part-specified') !== null,
          viewButton: [...document.querySelectorAll('button')].some((b) => b.textContent.trim() === ${JSON.stringify(T.viewButton)}),
          // The Volume row own figure, not the page as a whole.
          volumeTitle: (() => {
            const dt = [...document.querySelectorAll('dt')].find((e) => e.textContent.trim() === 'Volume')
            const dd = dt?.nextElementSibling
            return dd?.querySelector('span[title]')?.getAttribute('title') ?? null
          })(),
        }
      })()`)
      expect(page_.h3.includes('Geometry'), `no Geometry section; headings are ${JSON.stringify(page_.h3)}`)
      expect(page_.file, 'no #part-file section')
      expect(page_.identity, 'no #part-identity section')
      expect(page_.index.length >= 2, `the section index lists ${page_.index.length} sections`)
      expect(!page_.likeness, 'the part page still mounts #part-likeness, whose tables 0054 dropped')
      expect(!page_.specified, 'the part page still mounts #part-specified, whose PMI blobs nothing writes')
      // **Nothing drew, and nothing was fetched to draw with.** A part page opening is the open path,
      // and the open path reads metadata and pictures. The button is the whole of the exception.
      const drew = await page.evaluate(`performance.getEntriesByName('lapidary:viewer-first-frame').length`)
      expect(drew === 0, `the part page drew ${drew} frame(s) without anybody pressing view`)
      expect(page_.viewButton, `${part.sourcePath} is an STL, so the page must offer "${T.viewButton}"`)
      const meshes = net.urls().filter((u) => /variant=original|\.glb(\?|$)/.test(u))
      expect(meshes.length === 0, `opening the page fetched a model file: ${JSON.stringify(meshes.slice(0, 2))}`)
      // **Figures must not lie**, and since the pivot there is only one side of it to prove: this
      // figure is read from a mesh, so it carries the mark. The glyph beside it is aria-hidden, so the
      // title is what an assertion can read.
      expect(
        page_.volumeTitle !== null,
        `no figure in the Volume row of ${part.sourcePath} carries a provenance title`,
      )
      expect(
        page_.volumeTitle.startsWith(T.approximateTitle),
        `${part.sourcePath} is a mesh, so its volume must say it is derived from tessellated geometry; the figure says ${JSON.stringify(page_.volumeTitle)}`,
      )
      return `${part.sourcePath}: ${page_.index.length} sections (${page_.index.join(', ')}), no likeness and no PMI section, nothing drawn, volume ${JSON.stringify(page_.volumeTitle)}`
    },
  },
  {
    // **Press view, and the browser reads the part's own file.**
    //
    // This replaces `measure` and `section-explode-pmi`, and it is the one flow that proves the new
    // shape of the product end to end: a part page draws nothing until it is asked, and when it is
    // asked it fetches `download?variant=original` — the ingested bytes, byte for byte — and nothing
    // we produced. The two assertions are deliberately opposite, because either alone would pass on a
    // build that had quietly brought a pipeline back: a frame with no `variant=original` behind it
    // would mean something else supplied the geometry, and a `variant=original` with no frame would
    // mean it never drew.
    name: 'view-original',
    widths: [1440, 390],
    async run(page, ctx) {
      const part = await ctx.partLike(/alike\/flange-dn40-lp-3310-02\.stl$/)
      await page.go(`/parts/${part.id}`, `document.querySelector('h1, h2') !== null`)
      // Watched from here, so the fetch this asserts can only be the press's.
      const net = await watchRequests(page)
      const at = await page.byText('button', T.viewButton, { exact: true })
      expect(at !== null, `no button reads "${T.viewButton}" on ${part.sourcePath}`)
      await page.click(at.x, at.y)
      // The viewer is a lazy() chunk, so the press loads three.js and the file together. 60 s is for
      // SwiftShader on a loaded machine; the flange is 150 KB, so none of it is transfer.
      const drew = await firstFrame(page, 60_000)
      expect(drew > 0, `pressing "${T.viewButton}" never reported a first frame`)
      // The canvas is real, in the DOM, inside the labelled region a screen reader is given.
      const canvas = await page.evaluate(
        `document.querySelectorAll(${JSON.stringify(`${S.viewer} canvas`)}).length`,
      )
      expect(canvas === 1, `the view drew a frame but holds ${canvas} canvases`)
      const urls = net.urls()
      const original = urls.filter((u) => u.includes('download?variant=original'))
      expect(
        original.length > 0,
        `the view drew without fetching ?variant=original, so something else gave it geometry: ${JSON.stringify(urls.filter((u) => u.includes('/api/')).slice(0, 5))}`,
      )
      // **And nothing GLB-shaped.** Every one of these was a real URL on this application before the
      // pivot; a request matching any of them means a rung, a render or an export reached the viewer.
      const produced = urls.filter((u) =>
        /\.glb(\?|$)|variant=(l0|l1|l2)|\/rungs\/|tessellation|\/thumbnails?\b|variant=3mf|variant=stl/.test(u),
      )
      expect(
        produced.length === 0,
        `the view fetched something we produced rather than the original: ${JSON.stringify(produced.slice(0, 3))}`,
      )
      // It read the whole file and it is not a conversion: the response is the blob the revision names.
      const figures = await page.evaluate(`(() => {
        const dt = [...document.querySelectorAll('dt')].find((e) => e.textContent.trim() === 'Triangles')
        return (dt?.nextElementSibling?.textContent ?? '').trim()
      })()`)
      return `${part.sourcePath}: ${drew} first frame, ${canvas} canvas, ${original.length} fetch of ?variant=original and 0 of anything produced; the page reads ${JSON.stringify(figures)}`
    },
  },
  {
    name: 'delete',
    // Delete is final (owner, 2026-10-17): no restore, no removed list. So the victim is a part this
    // flow uploads itself, stamped so every run sends new bytes, and Sweep is never touched.
    async run(page, ctx) {
      const stamp = `${ctx.stamp}-${Date.now()}`
      const file = `hex-spacer-m4x20-lp-2145-01-e2e-${stamp}.stl`
      const folder = join(ctx.tmp, `e2e-delete-${stamp}`)
      stampInto(folder, [['example/parts/hex-spacer-m4x20-lp-2145-01.stl', file]], `Lapidary e2e delete ${stamp}`, ctx)
      const before = await ctx.countParts(ctx.uploaded.id)
      await page.go(grid(ctx.uploaded.id), 'document.querySelector("#parts") !== null')
      await pressUpload(page, folder)
      const landed = await page.settle(
        () => ctx.countParts(ctx.uploaded.id),
        (n) => n === before + 1,
        { timeout: 120_000, every: 1000 },
      )
      expect(landed === before + 1, `the upload to delete never landed: ${before} parts before, ${landed} after`)
      const { parts } = await ctx.get(`/api/libraries/${ctx.uploaded.id}/parts?limit=500`)
      const victim = parts.find((p) => (p.sourcePath ?? '').endsWith(file))
      expect(victim !== undefined, `no part in Uploaded came from ${file}`)
      let deleted = false
      try {
        await page.go(`/parts/${victim.id}`, `document.querySelector('button[popovertarget="${S.partMenu}"]') !== null`)
        await page.openMenu(S.partMenu)
        const pressed = await page.evaluate(`(() => {
          const b = [...document.querySelectorAll('#part-menu button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.deletePart)})
          if (!b) return false
          b.click()
          return true
        })()`)
        expect(pressed, `no "${T.deletePart}" button in #${S.partMenu}`)
        // The confirmation has to say the file stays before anything is sent.
        const asked = await page.settle(
          () => page.evaluate(`document.querySelector('[role="dialog"]')?.textContent ?? ''`),
          (t) => t.includes(T.deleteKeepsFile),
          { timeout: 10_000 },
        )
        expect(asked.includes(T.deleteKeepsFile), `the delete confirmation does not say "${T.deleteKeepsFile}": ${JSON.stringify(asked.slice(0, 200))}`)
        const confirmed = await page.evaluate(`(() => {
          const b = [...document.querySelectorAll('[role="dialog"] button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.deletePart)})
          if (!b) return false
          b.click()
          return true
        })()`)
        expect(confirmed, `the confirmation has no "${T.deletePart}" button`)
        // It invalidates the grid and navigates back to it, because the part no longer exists.
        await page.waitFor(`new URL(location.href).pathname === '/'`, 20_000)
        const gone = await fetch(`${args.api}/api/parts/${victim.id}`)
        expect(gone.status === 404, `the detail route answered ${gone.status} for a deleted part`)
        deleted = true
        const after = await ctx.countParts(ctx.uploaded.id)
        expect(after === before, `Uploaded held ${before} parts before the upload and ${after} after the delete`)
        return `${JSON.stringify(victim.name)} uploaded, confirmation says the file stays on disk, deleted, 404 from the detail route (${before} parts either side)`
      } finally {
        // Whatever went wrong above, the part this flow made does not outlive it.
        if (!deleted) await ctx.del(`/api/parts/${victim.id}`).catch(() => {})
      }
    },
  },
  {
    name: 'upload',
    async run(page, ctx) {
      // A whole folder, through the real control. The only input the app offers for models carries
      // `webkitdirectory`, so this is a *folder* upload with `webkitRelativePath` filled in, and the
      // categories it makes are part of what is under test. Handing `DOM.setFileInputFiles` the files
      // instead of the directory sets nothing at all — see `setFiles`, which now says so.
      const folder = join(ctx.tmp, `e2e-upload-${ctx.stamp}`)
      const inner = join(folder, UPLOAD_CATEGORY)
      stampInto(inner, UPLOAD_FILES, `Lapidary e2e ${ctx.stamp}`, ctx)
      const before = await ctx.countParts(ctx.uploaded.id)
      await page.go(grid(ctx.uploaded.id), 'document.querySelector("#parts") !== null')
      expect(await exists(page, S.fileInput), `no model file input matched ${S.fileInput}`)
      const before2 = before
      const chooser = await pressUpload(page, folder)
      // The transfer line is *transient*, and for a 1 KB fixture it can be gone before the first poll —
      // the first real run spent 30 s waiting for a line the upload had already finished with. So it is
      // reported when it appears and never required; the batch line below is the outcome that matters.
      // Matched on this line's own vocabulary all the same: taking the first `role="status"` in the DOM
      // would read the grid skeleton's "Loading parts…" as upload progress and call that a pass.
      const progress = await page.settle(
        () =>
          page.evaluate(`(() => {
            const want = ${JSON.stringify(W.upload)}
            const hits = [...document.querySelectorAll(${JSON.stringify(S.statusIn)})]
              .map((e) => e.textContent.trim())
              .filter((t) => want.some((w) => t.includes(w)))
            return { hits, all: [...document.querySelectorAll('#parts [role="status"]')].map((e) => e.textContent.trim().slice(0, 40)) }
          })()`),
        (r) => r.hits.length === 1,
        { timeout: 6000, every: 100 },
      )
      expect(
        progress.hits.length <= 1,
        `${progress.hits.length} lines matched the upload's wording, so the rig cannot tell which is the transfer line: ${JSON.stringify(progress.hits)}`,
      )
      const done = await page.settle(
        () => page.evaluate('document.body.textContent'),
        (t) => t.includes(T.uploadComplete),
        { timeout: 120_000, every: 500 },
      )
      expect(done.includes(T.uploadComplete), `the upload never said "${T.uploadComplete}"`)
      const after = await page.settle(
        () => ctx.countParts(ctx.uploaded.id),
        (n) => n > before,
        { timeout: 60_000, every: 1000 },
      )
      expect(after > before, `the upload never landed: ${before} parts before and after`)
      expect(after === before + 2, `a 2-file folder should add 2 parts; it added ${after - before}`)
      // The folder structure survived, which a flat drop of loose files could not have shown: both parts
      // are filed under the directory they came from.
      const filed = await ctx.get(`/api/libraries/${ctx.uploaded.id}/parts?limit=500`)
      const landed = filed.parts.filter((p) => (p.sourcePath ?? '').includes(UPLOAD_CATEGORY))
      expect(
        landed.length >= 2,
        `the uploaded parts are not filed under ${UPLOAD_CATEGORY}: ${JSON.stringify(filed.parts.map((p) => p.sourcePath).slice(-4))}`,
      )
      return `${before2} -> ${after} parts in Uploaded through the real control (chooser ${chooser.mode}), filed under ${UPLOAD_CATEGORY}; transfer line ${progress.hits.length ? JSON.stringify(progress.hits[0]) : 'too quick to catch'}`
    },
  },
  {
    name: 'scan-progress',
    async run(page, ctx) {
      // Sweep is re-scanned, not Empty. Scanning Empty would spend the only empty-state fixture, start
      // an 861 MiB ingest that then runs underneath every later flow and underneath
      // `open-timing.mjs`'s numbers, and make run 2's statuses differ from run 1's — the rig breaking
      // its own repeatability. A re-scan of Sweep walks the same tree, shows the same progress line and
      // settles all-skipped.
      const before = await ctx.countParts(ctx.sweep.id)
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      await page.openMenu(S.libraryMenu)
      const started = await page.evaluate(`(() => {
        const b = [...document.querySelectorAll('#library-menu button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.scanStart)})
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(started, `no "${T.scanStart}" button in #${S.libraryMenu}`)
      // ScanProgress has no role, no id and no aria-live, so its text is the only handle there is —
      // which is a finding in its own right, not a workaround.
      const line = await page.settle(
        () => page.evaluate(`document.querySelector('#parts')?.textContent ?? ''`),
        (t) => W.scanRunning.test(t),
        { timeout: 90_000, every: 400 },
      )
      const shown = W.scanRunning.exec(line)?.[0]
      expect(
        shown !== undefined,
        'a scan was started and #parts shows neither "Reading the folder…" nor "Scanning — N of M files."',
      )
      const finished = await page.settle(
        () => page.evaluate(`document.querySelector('#parts')?.textContent ?? ''`),
        (t) => W.scanDone.test(t),
        { timeout: 600_000, every: 2000 },
      )
      const done = W.scanDone.exec(finished)?.[0]
      expect(done !== undefined, 'the scan never said "Scan complete — …"')
      // Nothing new: the same bytes at the same paths are already here, so a second scan adds no part.
      const after = await ctx.countParts(ctx.sweep.id)
      expect(after === before, `a re-scan of an unchanged tree changed the count: ${before} -> ${after}`)
      expect(
        /already here/.test(done),
        `a re-scan should report everything already here; it says ${JSON.stringify(done)}`,
      )
      return `${JSON.stringify(shown)} -> ${JSON.stringify(done)}; ${before} parts either side`
    },
  },
  {
    name: 'empty-library',
    async run(page, ctx) {
      // The empty states, which nothing else covers — and the reason `scan-progress` above leaves this
      // library alone.
      expect(await ctx.countParts(ctx.empty.id) === 0, 'the Empty library is not empty any more')
      await page.go(grid(ctx.empty.id), `document.querySelector('#parts') !== null`)
      const body = await page.settle(
        () => page.evaluate('document.body.textContent'),
        (t) => t.includes('Nothing here yet'),
        { timeout: 20_000 },
      )
      expect(body.includes('Nothing here yet'), 'an empty library does not say "Nothing here yet"')
      // It offers the two ways out rather than only stating the fact.
      const offered = await page.evaluate(
        `[...document.querySelectorAll('button')].map((b) => b.textContent.trim()).filter((t) => t.includes('Upload a folder'))`,
      )
      expect(offered.length > 0, 'the first-run state offers no "Upload a folder" button')
      expect(
        (await cards(page)) === 0,
        'the empty library rendered a card',
      )
      return `"Nothing here yet", ${offered.length} upload affordance(s), 0 cards`
    },
  },
  {
    name: 'storage-panel',
    async run(page, ctx) {
      const instance = await ctx.get('/api/storage')
      const library = await ctx.get(`/api/libraries/${ctx.sweep.id}/storage`)
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      await openRail(page)
      // A <details> at the foot of the rail, closed by default, and its body renders only once the
      // grid has cards.
      const summary = await page.evaluate(`(() => {
        const d = [...document.querySelectorAll('details')].find((e) => e.querySelector('summary')?.textContent.trim().startsWith(${JSON.stringify(T.storageSummary)}))
        if (!d) return null
        d.open = true
        return d.querySelector('summary').textContent.trim()
      })()`)
      expect(summary !== null, `no <details> whose summary starts "${T.storageSummary}"`)
      await sleep(600)
      const body = await page.evaluate(`(() => {
        const d = [...document.querySelectorAll('details')].find((e) => e.querySelector('summary')?.textContent.trim().startsWith(${JSON.stringify(T.storageSummary)}))
        return d.textContent
      })()`)
      expect(/Sources/.test(body), `the storage panel body does not report sources: ${JSON.stringify(body.slice(0, 200))}`)
      // "Eviction is a different action with different wording and must never read as data loss."
      // These are the words that would break that rule, checked where a person would read them.
      const wrong = ['Delete cache', 'Delete derivatives', 'Erase cache', 'Delete the cache', 'Delete previews'].filter((w) => body.includes(w))
      expect(wrong.length === 0, `the storage wording reads as data loss: ${JSON.stringify(wrong)}`)
      if (/cache/i.test(body)) {
        expect(
          /Free cache space/.test(body),
          'the panel mentions a cache but offers no "Free cache space…" wording',
        )
      }
      return `${JSON.stringify(summary)}; instance keys ${Object.keys(instance).join(', ')}; library ${JSON.stringify(library).slice(0, 100)}`
    },
  },
]

// ---------------------------------------------------------------------------------------------------
// The runner.

const ctx = {
  root: new URL('../../', import.meta.url).pathname,
  tmp: mkdtempSync(join(tmpdir(), 'lapidary-e2e-files-')),
  stamp: String(Date.now()),
  sweep: seed.sweep,
  default: seed.default,
  uploaded: seed.uploaded,
  empty: seed.empty,
  async request(method, path, body) {
    const response = await fetch(args.api + path, {
      method,
      ...(body === undefined ? {} : { headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) }),
    })
    if (!response.ok) throw new Error(`${method} ${path} answered ${response.status}`)
    const payload = await response.text()
    return payload === '' ? {} : JSON.parse(payload)
  },
  get(path) {
    return ctx.request('GET', path)
  },
  post(path, body) {
    return ctx.request('POST', path, body ?? {})
  },
  del(path) {
    return ctx.request('DELETE', path)
  },
  /**
   * How many parts a library holds.
   *
   * `GET /api/libraries/{id}/parts` answers `{ parts, next }` and **no total** — the grid is keyset
   * paged and computes its own count line. So counting means walking the pages on `next`, and a flow
   * that compared a `.total` would be comparing `undefined` with `undefined` and passing for free.
   */
  async countParts(library) {
    let after
    let total = 0
    for (let page = 0; page < 40; page++) {
      const query = `limit=500${after ? `&after=${after}` : ''}`
      const { parts, next } = await ctx.get(`/api/libraries/${library}/parts?${query}`)
      total += parts.length
      if (next === null || next === undefined) return total
      after = next
    }
    throw new Error(`countParts walked 40 pages of ${library} without reaching the end`)
  },
  /**
   * Every part of Sweep, cached. Paged on `next` rather than trusting one `limit=500` read: Sweep holds
   * about 412 today, so a single page happens to be the whole thing, and a flow written against that
   * would quietly test a prefix the day the corpus slice grows.
   */
  async allParts() {
    if (ctx._parts === undefined) {
      const all = []
      let after
      for (let page = 0; page < 40; page++) {
        const { parts, next } = await ctx.get(
          `/api/libraries/${ctx.sweep.id}/parts?limit=500${after ? `&after=${after}` : ''}`,
        )
        all.push(...parts)
        if (next === null || next === undefined) break
        after = next
      }
      ctx._parts = all
    }
    return ctx._parts
  },
  /** One part of Sweep whose source path matches. */
  async partLike(pattern, optional = false) {
    const found = (await ctx.allParts()).find((p) => pattern.test(p.sourcePath ?? ''))
    if (found === undefined && !optional) {
      throw new Error(`no part in Sweep whose source path matches ${pattern}; the seed did not land`)
    }
    return found ?? null
  },
}

const rows = []

async function runFlow(flow, page, suffix) {
  const name = suffix ? `${flow.name}@${suffix}` : flow.name
  const began = Date.now()
  // Every flow at one width shares one browser profile, and the grid's page size, order, density and
  // layout live in `localStorage` under `lapidary.grid.v1.<libraryId>` rather than in the URL. So
  // `page-size` leaves 250 behind and `sort` leaves a new order, and without this `paging` would be
  // testing 250-per-page by accident and `--only paging` would not reproduce the full run's result.
  try {
    await page.send('Storage.clearDataForOrigin', { origin: args.web, storageTypes: 'local_storage' })
  } catch (error) {
    console.log(`  note  could not clear ${args.web}'s stored preferences: ${error.message}`)
  }
  let status = 'ok'
  let detail = ''
  try {
    detail = (await flow.run(page, ctx)) ?? ''
    // A pending flow still runs: if its fixtures stop holding, the rig should say so now rather than
    // when G6 arrives and finds them gone.
    if (flow.pending !== undefined) status = 'pending'
  } catch (error) {
    status = flow.pending === undefined ? 'failed' : 'pending-broken'
    detail = flow.pending === undefined ? error.message : `${flow.pending} — and its fixtures do not hold: ${error.message}`
  }
  let shot = null
  try {
    shot = await page.shot(name)
  } catch (error) {
    detail += ` (no screenshot: ${error.message})`
  }
  const ms = Date.now() - began
  rows.push({ name, status, ms, detail: String(detail).slice(0, 700), shot: shot?.split('/').pop() ?? null })
  const mark = status === 'ok' ? '  ok  ' : status === 'failed' ? 'FAILED' : status.padEnd(6)
  console.log(`${mark} ${name} ${ms} ms  ${detail}`)
}

const chosen = FLOWS.filter((f) => only === undefined || only.includes(f.name))
if (chosen.length === 0) throw new Error(`--only ${args.only} matched no flow`)
const shots = join(args.out, 'shots')

// 1440 × 900: every flow.
{
  const page = await session({ width: 1440, height: 900, base: args.web, shots })
  try {
    for (const flow of chosen) await runFlow(flow, page)
  } finally {
    await page.close()
  }
}

// 390 × 844: the seven surfaces whose narrow layout is its own design rather than a reflow. `hover` is
// off here on purpose — a phone cannot hover, and pretending otherwise would test a screen nobody has.
{
  const narrow = chosen.filter((f) => f.widths?.includes(390))
  if (narrow.length > 0) {
    const page = await session({ width: 390, height: 844, base: args.web, shots, hover: false })
    try {
      for (const flow of narrow) await runFlow(flow, page, '390')
    } finally {
      await page.close()
    }
  }
}

// The session below costs a Chrome and asserts something no single flow owns, so `--only` skips it:
// `--only paging` should run one flow, not open three browsers.
const wholeSuite = only === undefined

// One reduced-motion session over the grid and a part page: motion is a rule here, not a flourish.
//
// **Two rows no single flow owned have gone with the pivot**, and both for the same reason: a card has
// no hover handler at all since R1, so there is nothing to hover. `reduced-motion-no-spin` asserted
// that hovering a card started no turntable canvas under `prefers-reduced-motion`, and `hover-heap`
// measured the JS heap across 50 hovers to catch a leaked WebGL context per card. There is no
// turntable and no context: `Grid.test.tsx` asserts no `<canvas>` appears on hover at any setting, and
// `pictures` asserts that nothing is fetched per card. Those two are stronger than either row was,
// because they hold at the source rather than at the symptom. `cdp.mjs` keeps the CSS hover emulation,
// which the category tree's own row actions still need, and `heapAcrossHovers` stays unused rather
// than deleted — it is thirty lines and the next harness that wants a leak check wants exactly it.
if (wholeSuite) {
  const page = await session({ width: 1440, height: 900, base: args.web, shots, reducedMotion: true })
  try {
    for (const flow of chosen.filter((f) => ['grid-loads', 'part-detail'].includes(f.name))) {
      await runFlow(flow, page, 'reduced-motion')
    }
  } finally {
    await page.close()
  }
}

// Written here and from the exit handler both: a run killed between two flows should still leave the rows
// it gathered, because those rows are the only account of what it saw.
const save = () => writeFileSync(join(args.out, 'flows.json'), JSON.stringify(rows, null, 2))
process.on('exit', save)
save()
const failed = rows.filter((r) => r.status === 'failed' || r.status === 'pending-broken')
console.log(`\n${rows.length} rows, ${failed.length} failed${failed.length ? ': ' + failed.map((f) => f.name).join(', ') : ''}`)
process.exit(failed.length === 0 ? 0 : 1)
