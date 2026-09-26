#!/usr/bin/env node
// Captures of the sharing screens, one installation and one screen at a time, at both widths.
//
//   node scripts/e2e/sharing-shots.mjs --base http://127.0.0.1:34000 --out <dir> --tag a-sharing \
//     --view sharing
//
// Why a second driver beside `flows.mjs`: these screens exist only while three installations are in a
// particular state — an introduction disappears when it is accepted, "as somebody read it" holds only
// while the owner is away, a queue position needs a pull waiting behind another — so the captures have to
// be taken *inside* the scenario, at the moment each state exists, rather than in one pass at the end.
// `scripts/e2e/group.sh` is that scenario and calls this at each checkpoint.
//
// Everything about the browser comes from `cdp.mjs`: its transport rejects on a CDP error and throws on a
// page exception, its port is random so lanes cannot collide, and it emulates the CSS hover features that
// Tailwind's `@media (hover: hover)` utilities need — the category tree's own Share control is one of them.
//
// Views:
//   sharing          /sharing whole, as it stands
//   members-dialog   /sharing, with one share's "Change who it goes to" open
//   shared-folder    /sharing/shares/<id> — needs --share
//   share-dialog     the grid's category tree, with one category's Share dialog open — needs --library
//                    and --folder; --drop <name> unticks somebody, --ask ticks asking first, --confirm
//                    presses Share instead of Cancel
import { mkdirSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { session, sleep } from './cdp.mjs'

const { values: args } = parseArgs({
  options: {
    base: { type: 'string' },
    out: { type: 'string' },
    tag: { type: 'string' },
    view: { type: 'string' },
    share: { type: 'string' },
    library: { type: 'string' },
    folder: { type: 'string' },
    drop: { type: 'string' },
    ask: { type: 'boolean' },
    confirm: { type: 'boolean' },
    widths: { type: 'string' },
  },
})
for (const required of ['base', 'out', 'tag', 'view']) {
  if (args[required] === undefined) throw new Error(`--${required} is required`)
}
const widths = (args.widths ?? '1440,390').split(',').map((w) => Number(w.trim()))
mkdirSync(args.out, { recursive: true })

const DIALOG = '[role="dialog"][aria-modal="true"]'
const TREE = 'nav[aria-label="Categories"]'
const DRAWER_TOGGLE = 'button[aria-controls="drawer"]'
// The sharing page's own strings, matched as text because these regions label themselves with `useId()`.
const CHANGE_MEMBERS = 'Change who it goes to'
const SHARE_CONFIRM = 'Share'
const SHARE_CANCEL = 'Cancel'
const ASK_FIRST = 'Ask me before anyone pulls its files'

/** Press a button by its accessible name, inside an optional root. React's onClick needs no real event. */
const pressNamed = (page, name, root = 'document') =>
  page.evaluate(`(() => {
    const scope = ${root}
    if (!scope) return null
    const want = ${JSON.stringify(name)}
    const b = [...scope.querySelectorAll('button')].find(
      (e) => (e.getAttribute('aria-label') ?? e.textContent ?? '').trim() === want,
    )
    if (!b) return null
    b.click()
    return true
  })()`)

/** Below 768 the tree lives in `#drawer`, translated off screen: found by querySelector, never clickable. */
async function openRail(page) {
  if (page.width >= 768) return false
  const opened = await page.evaluate(`(() => {
    const t = document.querySelector(${JSON.stringify(DRAWER_TOGGLE)})
    if (!t || t.getAttribute('aria-expanded') === 'true') return false
    t.click()
    return true
  })()`)
  if (opened) await sleep(400)
  return opened
}

/**
 * Wait until the page has stopped saying "Loading…" — and carry on regardless when it has not.
 *
 * A screenshot of a page still loading is a finding, not a reason to abandon the run, and this script's
 * whole job is to bring back what the screens look like.
 */
async function settled(page) {
  try {
    await page.waitFor(`!document.body.textContent.includes('Loading…')`, 20_000)
  } catch {
    process.stdout.write(`  still loading at ${page.width} px, shot anyway\n`)
  }
  await sleep(700)
}

async function capture(width) {
  const page = await session({ width, height: width >= 768 ? 900 : 844, base: args.base, shots: args.out })
  const name = `${args.tag}@${width}`
  try {
    if (args.view === 'sharing' || args.view === 'members-dialog') {
      await page.go('/sharing', `document.querySelector('h2') !== null`)
      await settled(page)
      if (args.view === 'members-dialog') {
        if ((await pressNamed(page, CHANGE_MEMBERS)) === null) {
          throw new Error(`no "${CHANGE_MEMBERS}" control on /sharing — is anything shared here?`)
        }
        await page.waitFor(`document.querySelector(${JSON.stringify(DIALOG)}) !== null`, 10_000)
        await sleep(500)
      }
      await page.shot(name)
    } else if (args.view === 'shared-folder') {
      if (args.share === undefined) throw new Error('--share <peerShareId> is required for shared-folder')
      await page.go(`/sharing/shares/${args.share}`, `document.querySelector('h2') !== null`)
      await settled(page)
      await page.shot(name)
    } else if (args.view === 'share-dialog') {
      if (args.library === undefined || args.folder === undefined) {
        throw new Error('--library and --folder are required for share-dialog')
      }
      await page.go(`/?library=${args.library}`, `document.querySelector(${JSON.stringify(TREE)}) !== null`)
      await settled(page)
      await openRail(page)
      // The tree's per-row actions are `opacity-0 pointer-events-none` until the row is hovered, which a
      // coordinate click cannot reach and a `.click()` does not need.
      const label = `Share ${args.folder} with the people you are paired with`
      if ((await pressNamed(page, label, `document.querySelector(${JSON.stringify(TREE)})`)) === null) {
        throw new Error(`no category row named ${JSON.stringify(args.folder)} in the tree`)
      }
      await page.waitFor(`document.querySelector(${JSON.stringify(DIALOG)}) !== null`, 10_000)
      // The licence count arrives before the dialog is worth a picture: Share is disabled until it does.
      await page.waitFor(
        `(() => { const d = document.querySelector(${JSON.stringify(DIALOG)});
          return d !== null && !d.textContent.includes('Counting what this would offer') })()`,
        15_000,
      )
      await sleep(400)
      await page.shot(name)
      let changed = false
      if (args.drop !== undefined) {
        const dropped = await page.evaluate(`(() => {
          const d = document.querySelector(${JSON.stringify(DIALOG)})
          const l = [...d.querySelectorAll('label')].find((e) => e.textContent.includes(${JSON.stringify(args.drop)}))
          if (!l) return null
          const box = l.querySelector('input[type="checkbox"]')
          if (!box || !box.checked) return null
          box.click()
          return box.checked === false
        })()`)
        if (dropped !== true) throw new Error(`could not untick ${JSON.stringify(args.drop)} in the picker`)
        changed = true
      }
      if (args.ask === true) {
        const ticked = await page.evaluate(`(() => {
          const d = document.querySelector(${JSON.stringify(DIALOG)})
          const l = [...d.querySelectorAll('label')].find((e) => e.textContent.includes(${JSON.stringify(ASK_FIRST)}))
          if (!l) return null
          const box = l.querySelector('input[type="checkbox"]')
          if (!box || box.checked) return null
          box.click()
          return box.checked === true
        })()`)
        if (ticked !== true) throw new Error('could not tick asking first in the dialog')
        changed = true
      }
      if (changed) {
        await sleep(300)
        await page.shot(`${args.tag}-chosen@${width}`)
      }
      // Confirmed once, at the widest width only: the second width would share what the first already did.
      if (args.confirm === true && width === widths[0]) {
        if ((await pressNamed(page, SHARE_CONFIRM, `document.querySelector(${JSON.stringify(DIALOG)})`)) === null) {
          throw new Error('no Share button in the dialog')
        }
        await page.waitFor(`document.querySelector(${JSON.stringify(DIALOG)}) === null`, 15_000)
        process.stdout.write(`  shared ${args.folder} through the dialog\n`)
      } else {
        await pressNamed(page, SHARE_CANCEL, `document.querySelector(${JSON.stringify(DIALOG)})`)
      }
    } else {
      throw new Error(`unknown --view ${JSON.stringify(args.view)}`)
    }
    process.stdout.write(`  ${name}.png\n`)
  } finally {
    await page.close()
  }
}

let failed = 0
for (const width of widths) {
  try {
    await capture(width)
  } catch (error) {
    failed += 1
    process.stdout.write(`  FAILED ${args.tag}@${width}: ${error.message}\n`)
  }
}
process.exit(failed === 0 ? 0 : 1)
