#!/usr/bin/env node
// The flows: what a person does with this application, each one driven in a real browser against the
// real api, the real worker and the real database.
//
//   node scripts/e2e/flows.mjs --out <run dir> --web <url> --api <url> --seed <seed.json> [--only a,b]
//
// A flow is data — `{ name, route, widths, pending?, run(page, ctx) }` — and the runner does the same
// thing to every one of them: navigate fresh, run it inside its own try/catch, screenshot either way,
// append a row. So a flow cannot leave the next one wedged, a failure still produces the picture that
// explains it, and adding the twenty-first is writing one object.
//
// Two rules the flows keep, because a run has to be repeatable against one `--keep` stack and stage 6
// asks for two runs with identical statuses:
//
//   * a flow that changes anything asserts a **delta**, never an absolute count, and puts back what it
//     took (`remove-restore` restores, `saved-filter` deletes its filter, `upload` stamps its bytes so
//     the same file is never sent twice);
//   * every selector a flow depends on is a named entry in `S` below, so the goal's "rename one
//     selector and watch that flow alone fail" test is a one-line edit.
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { session, setFiles, sleep } from './cdp.mjs'

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
  cardThumb: 'article[aria-labelledby^="part-name-"] img[alt^="Rendered preview of "]',
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
  measureBar: '[role="toolbar"][aria-label="Measure"]',
  // `p[aria-live="polite"]` alone is ambiguous: the viewer has its own "Loading more detail…" line
  // earlier in the DOM. The reading is the toolbar's immediate sibling.
  measureReading: '[role="toolbar"][aria-label="Measure"] + p[aria-live="polite"]',
  sectionBar: '[role="toolbar"][aria-label="Section"]',
  pmiSection: '#part-specified',
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
  pointToPoint: 'Point to point',
  showInView: 'Show in the 3D view',
  explode: 'Explode',
  removeFromLibrary: 'Remove from library',
  restore: 'Restore',
  scanStart: 'Scan the ingest folder',
  storageSummary: 'Storage',
  thisInstallation: 'This installation',
  uploadComplete: 'Upload complete —',
  // The L2 wait. `open-timing.mjs` parses this literal out of strings.ts for the same reason.
  loadingMesh: 'Loading the full-detail mesh',
  // The provenance title on a tessellated figure. "Measurement must not lie": this is the mark, and
  // the `≈` beside it is aria-hidden, so the mark is what an assertion must read.
  approximateTitle: 'Derived from tessellated',
  // Its opposite, on a figure read from a B-rep entity. The two together are "measurement must not lie".
  analyticTitle: 'Read from an analytic CAD entity',
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
    name: 'thumbnails',
    async run(page, ctx) {
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      // `content-visibility: auto` on every <li>, so an off-screen card's <img> has a src but is not
      // laid out. Counting `src` is therefore the right question and `checkVisibility` is not.
      const got = await page.settle(
        () =>
          page.evaluate(`(() => {
            const all = [...document.querySelectorAll(${JSON.stringify(S.card)})]
            const drawn = all.filter((a) => a.querySelector('img[alt^="Rendered preview of "]'))
            return {
              cards: all.length,
              drawn: drawn.length,
              inline: drawn.filter((a) => a.querySelector('img').src.startsWith('data:')).length,
              none: all.length - drawn.length,
            }
          })()`),
        (r) => r.cards > 0 && r.none === 0,
        { timeout: 30_000 },
      )
      expect(got.none === 0, `${got.none} of ${got.cards} cards show no rendered preview`)
      // Inline `data:` urls are the whole reason a page of cards costs one request: a card that
      // fetched its own picture would be a per-card round trip nobody asked for.
      expect(got.inline === got.drawn, `${got.drawn - got.inline} thumbnails are not inline data: urls`)
      return `${got.drawn} of ${got.cards} cards, all inline data: urls`
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
      expect(after.length > 0, 'searching for "flange" found nothing; alike/ holds four flanges')
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
      for (const want of ['stl', 'step', 'igs', 'obj', '3mf']) {
        expect(formats.includes(want), `the format facet has no \`${want}\`: ${formats.join(', ')}`)
      }
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      await openRail(page)
      // A <button aria-pressed>, not a checkbox, and its accessible name is `STEP, 6 parts` — so the
      // visible inner <span> is what identifies it.
      const pressed = await page.evaluate(`(() => {
        const section = document.querySelector(${JSON.stringify(S.formatFacet)})
        if (!section) return 'no format facet section'
        const b = [...section.querySelectorAll('button')].find((e) => e.querySelector('span')?.textContent === 'STEP')
        if (!b) return 'no STEP button'
        b.click()
        return b.getAttribute('aria-label')
      })()`)
      expect(!pressed.startsWith('no '), `${pressed} in ${S.formatFacet}`)
      await page.waitFor(`new URL(location.href).searchParams.get('format') === 'step'`, 10_000)
      const n = await cardsSettled(page, (v) => v > 0 && v < 50)
      const step = facets.formats.find((f) => f.value === 'step')
      expect(n > 0, 'filtering to STEP showed no cards')
      // `count` is null past the server's exact-count threshold, and the UI then shows no number at all.
      // Six STEP fixtures is far under it, so an exact match is the real assertion — but a null must not
      // fail against correct behaviour.
      if (step.count === null) {
        expect(n <= 50, `${n} cards on one page of 50`)
      } else {
        expect(n === step.count, `${n} cards shown, but the facet counts ${step.count} STEP parts`)
      }
      return `${formats.length} formats (${formats.join(', ')}); STEP ${JSON.stringify(pressed)} shows ${n}`
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
      const before = await names(page)
      const reordered = await page.settle(
        () => names(page),
        (list) => list.length > 0 && list.join('\u0000') !== before.join('\u0000'),
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
      await page.go(grid(ctx.sweep.id, '&format=step'), gridReady)
      await cardsSettled(page)
      await openRail(page)
      const opened = await page.evaluate(`(() => {
        const b = [...document.querySelectorAll('button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.saveThisFilter)})
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(opened, `filtering to STEP showed no "${T.saveThisFilter}" button`)
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
      expect(mine.search.format === 'step', `it saved ${JSON.stringify(mine.search)} rather than format=step`)
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
      return `saved ${JSON.stringify(name)} as format=step, marked current, then removed`
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
      await page.go(grid(ctx.sweep.id), gridReady)
      await cardsSettled(page)
      const card = await page.rect(S.cardThumb, 0)
      expect(card !== null, `no card thumbnail matched ${S.cardThumb}`)
      await page.click(card.x, card.y)
      // Two shapes by width, and the 1280px boundary is a matchMedia the device-metrics override
      // flips: a non-modal <aside> pane when there is room beside the grid, a modal dialog when not.
      const wide = page.width >= 1280
      const surface = wide ? S.quickLookPane : S.dialog
      await page.waitFor(`document.querySelector(${JSON.stringify(surface)}) !== null`, 20_000)
      await page.waitFor(`new URL(location.href).searchParams.get('part') !== null`, 8000)
      const drew = await firstFrame(page)
      expect(drew > 0, 'the quick look opened but the viewer never reported a first frame')
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
      return `${wide ? 'pane' : 'modal dialog'} at ${page.width}px, ${drew} first frame(s), closed and ?part cleared`
    },
  },
  {
    name: 'part-detail',
    widths: [1440, 390],
    async run(page, ctx) {
      const part = await ctx.partLike(/pmi/i)
      await page.go(`/parts/${part.id}`, `document.querySelector('h1, h2') !== null`)
      const drew = await firstFrame(page)
      expect(drew > 0, `the part page for ${part.sourcePath} never drew a first frame`)
      // The section index is built by a MutationObserver over the sections that actually rendered, so
      // it is the page's own account of itself.
      const page_ = await page.evaluate(`(() => {
        const h3 = [...document.querySelectorAll('h3')].map((h) => h.textContent.trim())
        return {
          h3,
          index: [...document.querySelectorAll('nav[aria-label="On this page"] a')].map((a) => a.textContent.trim()),
          file: document.querySelector('#part-file') !== null,
          identity: document.querySelector('#part-identity') !== null,
          // The Volume row's own figure, not the page as a whole. A STEP part's triangle count counts
          // tessellated primitives and cannot be analytic -- the revision schema says so, and declines
          // to give it a _source column -- so an approximate mark somewhere on this page is correct.
          // "Analytic values from B-rep entities where available" is about figures like this one.
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
      // Measurement must not lie, and this is the half a B-rep fixture can prove: the volume of a STEP
      // solid is read from the entity, not derived from a mesh. The mesh half is `measure`, which
      // asserts the tessellated mark on a reading taken from an STL.
      expect(
        page_.volumeTitle !== null,
        `no figure in the Volume row of ${part.sourcePath} carries a provenance title`,
      )
      expect(
        page_.volumeTitle.startsWith(T.analyticTitle),
        `${part.sourcePath} is B-rep, so its volume must be read from an analytic entity; the figure says ${JSON.stringify(page_.volumeTitle)}`,
      )
      return `${part.sourcePath}: ${page_.index.length} sections (${page_.index.join(', ')}), volume ${JSON.stringify(page_.volumeTitle)}`
    },
  },
  {
    name: 'measure',
    async run(page, ctx) {
      // A mesh, deliberately: its reading is the one that must be labelled approximate.
      const part = await ctx.partLike(/alike\/flange-dn40-lp-3310-02\.stl$/)
      await page.go(`/parts/${part.id}`, `document.querySelector(${JSON.stringify(S.viewer)}) !== null`)
      await firstFrame(page)
      await page.waitFor(`document.querySelector(${JSON.stringify(S.measureBar)}) !== null`, 20_000)
      const tools = await page.evaluate(
        `[...document.querySelector(${JSON.stringify(S.measureBar)}).querySelectorAll('button[aria-pressed]')].map((b) => b.textContent.trim())`,
      )
      expect(tools.includes(T.pointToPoint), `the measure bar offers ${JSON.stringify(tools)}`)
      await page.evaluate(`[...document.querySelector(${JSON.stringify(S.measureBar)}).querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(T.pointToPoint)}).click()`)
      // L2 has to arrive before a pick means anything, and the line says so until it has.
      await page.settle(
        () => text(page, S.measureReading),
        (t) => t !== null && !t.includes(T.loadingMesh),
        { timeout: 40_000, every: 600 },
      )
      const box = await page.evaluate(`(() => {
        const r = document.querySelector(${JSON.stringify(S.viewer)}).getBoundingClientRect()
        return { x: r.x, y: r.y, w: r.width, h: r.height }
      })()`)
      // Pairs across the flange's face, tried until one reads: a pick that misses the solid picks
      // nothing and the prompt stays. The picks are cleared between tries by toggling the tool, and
      // the coordinates are the ones the UI pass found land on this part.
      const pairs = [
        [[0.414, 0.555], [0.59, 0.555]],
        [[0.42, 0.62], [0.6, 0.62]],
        [[0.38, 0.5], [0.64, 0.5]],
        [[0.45, 0.33], [0.6, 0.66]],
      ]
      let read = ''
      for (const [[ax, ay], [bx, by]] of pairs) {
        // pointerdown/pointerup with a click-slop threshold, so real input events and not el.click().
        await page.click(box.x + box.w * ax, box.y + box.h * ay)
        await sleep(350)
        await page.click(box.x + box.w * bx, box.y + box.h * by)
        await sleep(900)
        read = (await text(page, S.measureReading)) ?? ''
        if (/\d+\.\d{3} mm/.test(read)) break
        for (let i = 0; i < 2; i++) {
          await page.evaluate(`[...document.querySelector(${JSON.stringify(S.measureBar)}).querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(T.pointToPoint)}).click()`)
          await sleep(250)
        }
      }
      expect(/\d+\.\d{3} mm/.test(read), `no pick gave a reading in mm; the line says ${JSON.stringify(read)}`)
      // The mark, not the `≈` glyph: that glyph is aria-hidden, and the provenance is in the title.
      const marked = await page.evaluate(
        `document.querySelector(${JSON.stringify(S.measureReading)}).querySelector('span[title^="${T.approximateTitle}"]') !== null`,
      )
      expect(
        marked,
        `${part.sourcePath} is a mesh, so its reading must carry the "${T.approximateTitle}…" mark; it reads ${JSON.stringify(read)}`,
      )
      return `${JSON.stringify(read)}, marked approximate; tools ${tools.join(', ')}`
    },
  },
  {
    name: 'section-explode-pmi',
    async run(page, ctx) {
      const pmi = await ctx.partLike(/pmi/i)
      const detail = await ctx.get(`/api/parts/${pmi.id}`)
      // check-plain.sh's assertions, kept: this is what proves the real kernel ran, not the mock.
      expect(detail.pmi != null, 'the PMI cylinder carries no pmi blob')
      expect(detail.structure != null, 'the PMI cylinder carries no structure')
      expect(detail.entities != null, 'the PMI cylinder carries no entities')
      expect(
        typeof detail.kernelVersion === 'string' && detail.kernelVersion.length > 0,
        `no kernel version on the PMI cylinder: ${JSON.stringify(detail.kernelVersion)}`,
      )
      await page.go(`/parts/${pmi.id}`, `document.querySelector(${JSON.stringify(S.viewer)}) !== null`)
      await firstFrame(page)
      await page.waitFor(`document.querySelector(${JSON.stringify(S.sectionBar)}) !== null`, 20_000)
      // Section: Off at rest, then X/Y/Z, and picking an axis grows the position slider and Flip.
      const axes = await page.evaluate(
        `[...document.querySelector(${JSON.stringify(S.sectionBar)}).querySelectorAll('button[aria-pressed]')].map((b) => b.textContent.trim())`,
      )
      for (const want of ['Off', 'X', 'Y', 'Z']) {
        expect(axes.includes(want), `the section bar offers ${JSON.stringify(axes)}, without ${want}`)
      }
      await page.evaluate(`[...document.querySelector(${JSON.stringify(S.sectionBar)}).querySelectorAll('button')].find((b) => b.textContent.trim() === 'Z').click()`)
      await page.waitFor(
        `document.querySelector(${JSON.stringify(S.sectionBar)}).querySelector('input[type="range"][aria-label="Where the cut is"]') !== null`,
        8000,
      )
      const at = await page.evaluate(
        `document.querySelector(${JSON.stringify(S.sectionBar)}).querySelector('input[type="range"]').value`,
      )
      await page.setControl(`${S.sectionBar} input[type="range"]`, '750', 'input')
      // PMI is not in the viewer bar: it is a toggle inside the specified-dimensions section.
      const found = await page.evaluate(`(() => {
        const s = document.querySelector(${JSON.stringify(S.pmiSection)})
        if (!s) return 'no ${S.pmiSection} section'
        const b = [...s.querySelectorAll('button[aria-pressed]')].find((e) => e.textContent.trim() === ${JSON.stringify(T.showInView)})
        if (!b) return 'no toggle reading "${T.showInView}"'
        b.click()
        return 'clicked'
      })()`)
      expect(found === 'clicked', `the PMI toggle: ${found}`)
      // Read after React has re-rendered, not in the same tick as the click — reading `aria-pressed`
      // immediately reported the old value and failed against a toggle that worked.
      const pmiToggle = await page.settle(
        () =>
          page.evaluate(`(() => {
            const b = [...document.querySelectorAll(${JSON.stringify(`${S.pmiSection} button[aria-pressed]`)})].find((e) => e.textContent.trim() === ${JSON.stringify(T.showInView)})
            return b ? b.getAttribute('aria-pressed') : null
          })()`),
        (v) => v === 'true',
        { timeout: 8000 },
      )
      expect(pmiToggle === 'true', `the PMI toggle stayed aria-pressed=${pmiToggle} after being clicked`)
      // The labels are DOM over the canvas, not drawn into it, which is why they are assertable.
      const labels = await page.evaluate(
        `document.querySelectorAll(${JSON.stringify(`${S.pmiSection} [role="list"] li`)}).length`,
      )
      expect(labels > 0, 'the specified-dimensions section lists no tolerances')
      // Explode is a labelled range and only exists for a drawn assembly, so it is looked for on the
      // assembly and its absence here is correct rather than a failure.
      const assembly = await ctx.partLike(/assembly/i, true)
      let explode = 'no assembly fixture'
      if (assembly !== null) {
        await page.go(`/parts/${assembly.id}`, `document.querySelector(${JSON.stringify(S.viewer)}) !== null`)
        await firstFrame(page)
        explode = await page.settle(
          () =>
            page.evaluate(`(() => {
              const l = [...document.querySelectorAll('label')].find((e) => e.textContent.trim() === ${JSON.stringify(T.explode)})
              return l?.querySelector('input[type="range"]') ? 'present' : 'absent'
            })()`),
          (v) => v === 'present',
          { timeout: 20_000 },
        )
      }
      return `kernel ${detail.kernelVersion}; section axes ${axes.join('/')} (cut at ${at} -> 750), ${labels} PMI rows shown, explode ${explode}`
    },
  },
  {
    name: 'remove-restore',
    // Stage 4's narrow set is grid, quick look, part detail, **removed** and sharing, and /removed is this
    // flow's. Safe at both widths because it restores what it took.
    widths: [1440, 390],
    async run(page, ctx) {
      // Removed and put back inside the flow, so a second run sees the same library.
      const victim = await ctx.partLike(/scaled-115/)
      const before = await ctx.countParts(ctx.sweep.id)
      await page.go(`/parts/${victim.id}`, `document.querySelector('h1, h2') !== null`)
      // No confirmation, deliberately: removal is reversible and the hint under the button says so.
      // The control is in the part's own actions menu, whose children are display:none until opened.
      await page.openMenu(S.partMenu)
      const pressed = await page.evaluate(`(() => {
        const b = [...document.querySelectorAll('#part-menu button')].find((e) => e.textContent.trim().startsWith(${JSON.stringify(T.removeFromLibrary)}))
        if (!b) return false
        b.click()
        return true
      })()`)
      expect(pressed, `no "${T.removeFromLibrary}" button in #${S.partMenu}`)
      // It invalidates the grid and navigates back to it.
      await page.waitFor(`new URL(location.href).pathname === '/'`, 20_000)
      // Soft, never implicit: the part is still readable and a purge is a different action.
      const still = await ctx.get(`/api/parts/${victim.id}`)
      expect(still.id === victim.id, 'a removed part should still be readable; delete is soft')
      await page.go(`/removed?library=${ctx.sweep.id}`, `document.querySelector('h2') !== null`)
      const listed = await page.settle(
        () => page.evaluate('document.body.textContent'),
        (t) => t.includes(victim.name),
        { timeout: 20_000 },
      )
      expect(listed.includes(victim.name), `${JSON.stringify(victim.name)} is removed but /removed does not name it`)
      // The restore control is a plain button in the row; there is no aria-label and no id. So it has to
      // be found inside the row that names *this* part — the first Restore in the document belongs to
      // whichever part /removed happens to list first, and restoring somebody else's would leave this
      // flow's victim removed and quietly change the library for every run after it.
      const restored = await page.evaluate(`(() => {
        const row = [...document.querySelectorAll('li')].find((li) => li.textContent.includes(${JSON.stringify(victim.name)}))
        if (!row) return 'no row names it'
        const b = [...row.querySelectorAll('button')].find((e) => e.textContent.trim() === ${JSON.stringify(T.restore)})
        if (!b) return 'its row has no Restore button'
        b.click()
        return 'clicked'
      })()`)
      expect(restored === 'clicked', `${JSON.stringify(victim.name)} on /removed: ${restored}`)
      const after = await page.settle(
        () => ctx.countParts(ctx.sweep.id),
        (n) => n === before,
        { timeout: 30_000, every: 1000 },
      )
      if (after !== before) {
        // Belt and braces: the next run must not start a part short.
        await ctx.post(`/api/parts/${victim.id}/restore`, {})
        fail(`the library held ${before} parts and holds ${after} after the restore`)
      }
      return `${JSON.stringify(victim.name)} removed, listed on /removed, restored (${before} parts either side)`
    },
  },
  {
    name: 'upload',
    async run(page, ctx) {
      // Through the real control with `DOM.setFileInputFiles`: a file input's value cannot be set from
      // script, so anything else would not be the upload a person does. Note what that costs — a
      // CDP-injected file has an empty `webkitRelativePath`, so `lib/upload.ts` reads it as a flat
      // drop of loose files and the upload makes no category. That is correct, so nothing here
      // asserts a folder.
      const file = join(ctx.tmp, `bracket-lp-1042-03-e2e-${ctx.stamp}.stl`)
      const bytes = readFileSync(join(ctx.root, 'fixtures/bracket-lp-1042-03.stl'))
      // A per-run stamp in the 80-byte header: identical bytes would be `Skipped`, and a flow that
      // only passes the first time fails the rig's own repeatability test.
      Buffer.from(`Lapidary e2e ${ctx.stamp}`.slice(0, 79).padEnd(80, '\0'), 'binary').copy(bytes, 0)
      writeFileSync(file, bytes)
      const before = await ctx.countParts(ctx.governed.id)
      await page.go(grid(ctx.governed.id), 'document.querySelector("#parts") !== null')
      expect(await exists(page, S.fileInput), `no model file input matched ${S.fileInput}`)
      await setFiles(page, S.fileInput, [file])
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
        () => ctx.countParts(ctx.governed.id),
        (n) => n > before,
        { timeout: 60_000, every: 1000 },
      )
      expect(after > before, `the upload never landed: ${before} parts before and after`)
      return `${before} -> ${after} parts in Governed; transfer line ${progress.hits.length ? JSON.stringify(progress.hits[0]) : 'too quick to catch for a 1 KB file'}`
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
  {
    name: 'sharing',
    widths: [1440, 390],
    async run(page, ctx) {
      const identity = await ctx.get('/api/sharing/identity')
      expect(
        typeof identity.deviceId === 'string' && identity.deviceId.length > 0,
        'no device id to show; the peer role did not come up',
      )
      await page.go('/sharing', `document.querySelector('h2') !== null`)
      const regions = await page.settle(
        () =>
          page.evaluate(`(() => ({
            h3: [...document.querySelectorAll('h3')].map((h) => h.textContent.trim()),
            body: document.body.textContent,
          }))()`),
        (r) => r.h3.includes(T.thisInstallation) && r.body.includes(identity.deviceId.slice(0, 8)),
        { timeout: 20_000 },
      )
      expect(regions.h3.includes(T.thisInstallation), `the sharing page's regions are ${JSON.stringify(regions.h3)}`)
      expect(
        regions.body.includes(identity.deviceId.slice(0, 8)),
        `the page never shows this installation's device id (${identity.deviceId.slice(0, 11)}…)`,
      )
      // Off by default and it stays off: nothing here pairs anybody.
      const peers = await ctx.get('/api/sharing/peers')
      expect(Array.isArray(peers), 'GET /api/sharing/peers did not answer a list')
      expect(peers.length === 0, `${peers.length} peers are paired; this rig pairs nobody`)
      return `device ${identity.deviceId.slice(0, 11)}…, regions: ${regions.h3.join(' / ')}, ${peers.length} peers`
    },
  },
  {
    name: 'duplicates',
    widths: [1440, 390],
    pending:
      "Waiting on two merges, both owned. G6 brings the /duplicates page; G2 (Shape profiles in the " +
      "worker) brings the producer — nothing on main writes part_shape yet, so every part is unprofiled " +
      'and only the identical-by-hash half of likeness can answer. What runs below is real and asserted, ' +
      'and the near-duplicate and page assertions arm themselves the moment each lands, so this flag is ' +
      'the only edit either one needs.',
    async run(page, ctx) {
      // The three fixtures, and what each of them is *for*. This is the assertion `alike/` was seeded to
      // make, and `docs/phase-6.md`'s claim stated as a test: a rotation is a near-duplicate because the
      // descriptor is rotation-invariant, and a 15 % scale is not, by design.
      const alike = (await ctx.allParts()).filter((p) => (p.sourcePath ?? '').startsWith('alike/'))
      expect(alike.length === 4, `alike/ should hold the flange and its three variants; found ${alike.length}`)
      const of = (suffix) => alike.find((p) => p.sourcePath === `alike/flange-dn40-lp-3310-02${suffix}.stl`)
      const original = of('')
      const copy = of('-second-copy')
      const rotated = of('-rotated')
      const scaled = of('-scaled-115')
      for (const [what, part] of [['the original', original], ['-second-copy', copy], ['-rotated', rotated], ['-scaled-115', scaled]])
        expect(part !== undefined, `no ${what} fixture in alike/: ${JSON.stringify(alike.map((p) => p.sourcePath))}`)

      const likeness = await ctx.get(`/api/parts/${original.id}/likeness`)
      const ids = (list) => (list ?? []).map((p) => p.id)

      // Identical, and this half needs no profile at all — it is the parts sharing this one's current
      // source `blake3`. `library_holds` joins on source_path AND hash, so the same bytes at a second path
      // are a second part rather than a skipped job, which is the case an ingest-time hash check cannot
      // make and the reason `-second-copy` is in the fixtures.
      expect(
        ids(likeness.identical).includes(copy.id),
        `-second-copy has the same bytes at another path and must come back identical; identical holds ${ids(likeness.identical).length} parts`,
      )

      const clusters = await ctx.get(`/api/libraries/${ctx.sweep.id}/duplicates`)
      expect(Array.isArray(clusters.clusters), 'GET /duplicates did not answer a cluster list')
      expect(
        clusters.clusters.some((c) => c.identical && ids(c.parts).includes(copy.id)),
        `no identical cluster holds -second-copy; ${clusters.clusters.length} clusters, ${clusters.unprofiled} unprofiled`,
      )
      const folds = await ctx.get(`/api/libraries/${ctx.sweep.id}/folds`)
      expect(Array.isArray(folds), 'GET /folds did not answer a list')

      // The near-duplicate half, which arms itself. Nothing on main writes `part_shape` today — G3 built
      // the reads, W0 the types, and G2 ('Shape profiles in the worker') is building the producer — so
      // `profiled` is false, `nearDuplicates` and `similar` are empty, and the route saying so is correct
      // rather than broken. The moment a producer lands, `profiled` turns true and these three become the
      // assertions `alike/` was seeded to make: `docs/phase-6.md`'s rotation invariance, stated as a test.
      let near = 'unprofiled, so the near-duplicate half cannot be exercised yet'
      if (likeness.profiled) {
        expect(
          ids(likeness.nearDuplicates).includes(rotated.id),
          `-rotated is the same solid stood on a different axis and must be a near-duplicate; nearDuplicates holds ${ids(likeness.nearDuplicates).length}`,
        )
        // A detector that called 15 % larger a near-duplicate would be wrong, and this catches it.
        expect(
          !ids(likeness.nearDuplicates).includes(scaled.id),
          '-scaled-115 is 15 % larger, far outside the ln(1.02) band, and must NOT be a near-duplicate',
        )
        expect(
          ids(likeness.similar).includes(scaled.id) || ids(likeness.variants).includes(scaled.id),
          `-scaled-115 should come back similar or a variant; it is in neither (similar ${ids(likeness.similar).length}, variants ${ids(likeness.variants).length})`,
        )
        near = 'rotated is near, scaled-115 is similar and not near'
      } else {
        // The honest assertion while there is no producer: the route must say "not compared yet" rather
        // than imply nothing is alike, and the queue must own up to the whole library being unprofiled.
        expect(
          ids(likeness.nearDuplicates).length === 0 && ids(likeness.similar).length === 0,
          `profiled is false but the route returned ${ids(likeness.nearDuplicates).length} near and ${ids(likeness.similar).length} similar parts, which it cannot know`,
        )
        expect(
          clusters.unprofiled >= ctx.sweep.parts - 1,
          `nothing profiles parts yet, so /duplicates should report about ${ctx.sweep.parts} unprofiled; it says ${clusters.unprofiled}`,
        )
      }

      // The page. It turns itself on: while G6 is not on main the route renders nothing recognisable and
      // this reports what it found, and the day it does render, these become real assertions with no edit
      // here — so landing G6 leaves only the `pending` flag above to remove.
      await page.go('/duplicates', `document.readyState === 'complete'`)
      const hasPage = await page.evaluate(
        `[...document.querySelectorAll('h1, h2')].some((h) => /duplicate/i.test(h.textContent))`,
      )
      if (!hasPage) {
        return `API only (no /duplicates page yet): identical-by-hash correct, ${clusters.clusters.length} cluster(s), ${clusters.unprofiled} unprofiled, ${folds.length} fold(s); ${near}`
      }
      const shown = await page.settle(
        () => page.evaluate('document.body.textContent'),
        (t) => t.includes(original.name),
        { timeout: 20_000 },
      )
      expect(shown.includes(original.name), 'the /duplicates page does not name the flange it clustered')
      return `page and API agree: ${clusters.clusters.length} cluster(s), ${clusters.unprofiled} unprofiled, ${folds.length} fold(s); ${near}`
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
  governed: seed.governed,
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
  async countParts(library, state) {
    let after
    let total = 0
    for (let page = 0; page < 40; page++) {
      const query = `limit=500${after ? `&after=${after}` : ''}${state ? `&state=${state}` : ''}`
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

// 390 × 844: the five surfaces whose narrow layout is its own design rather than a reflow. `hover` is
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

// The three sessions below cost a Chrome each and assert things no single flow owns, so `--only` skips
// them: `--only measure` should run one flow, not open four browsers and fail on an unrelated heap reading.
const wholeSuite = only === undefined

// One reduced-motion session over the grid and a part page: motion is a rule here, not a flourish.
if (wholeSuite) {
  const page = await session({ width: 1440, height: 900, base: args.web, shots, reducedMotion: true })
  try {
    for (const flow of chosen.filter((f) => ['grid-loads', 'part-detail'].includes(f.name))) {
      await runFlow(flow, page, 'reduced-motion')
    }
    await page.go(grid(ctx.sweep.id), gridReady)
    await cardsSettled(page)
    const card = await page.rect(S.card, 2)
    if (card !== null) {
      await page.mouse(card.x, card.y - 40)
      await sleep(1400)
      // The turntable injects a <canvas> into the card's well when it spins, and nothing else does.
      const spinning = await page.evaluate(`document.querySelector('${S.card} canvas') !== null`)
      rows.push({
        name: 'reduced-motion-no-spin',
        status: spinning ? 'failed' : 'ok',
        ms: 0,
        detail: spinning
          ? 'hovering a card started a turntable canvas with prefers-reduced-motion: reduce'
          : 'hovering a card starts no canvas under reduced motion',
        shot: await page.shot('reduced-motion-hover').then((p) => p.split('/').pop(), () => null),
      })
    }
  } finally {
    await page.close()
  }
}

// The heap across 50 hovers, which is the one measurement a screenshot cannot make.
if (wholeSuite) {
  const page = await session({ width: 1440, height: 900, base: args.web, shots })
  try {
    await page.go(grid(ctx.sweep.id), gridReady)
    await cardsSettled(page)
    const heap = await page.heapAcrossHovers(50, S.card)
    const grew = heap.afterKB === null ? null : heap.afterKB - heap.beforeKB
    rows.push({
      name: 'hover-heap',
      // 40 MB after a collected heap is generous and still catches a context that is never released:
      // 50 leaked WebGL contexts are an order of magnitude more than that.
      status: grew !== null && grew < 40_000 ? 'ok' : 'failed',
      ms: 0,
      detail: `${heap.hovers} hovers over ${heap.cards} cards: ${heap.beforeKB} KB -> ${heap.afterKB} KB (${grew === null ? 'no reading' : `${grew > 0 ? '+' : ''}${grew} KB`})`,
      shot: null,
    })
    console.log(`  heap ${heap.beforeKB} -> ${heap.afterKB} KB across ${heap.hovers} hovers`)
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
