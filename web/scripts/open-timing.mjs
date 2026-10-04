#!/usr/bin/env node
// How long a part takes to open, measured the way the Phase 3 exit measured it (`docs/ROADMAP.md`),
// against a running stack. `DATA.md` §2.5 has the targets and how to run this.
//
// Headless Chrome at 1440 × 900 with a throwaway profile, driven over the DevTools protocol with
// nothing but Node's built-ins.
//
// **Two numbers now, because the open path and the 3D view are two different presses** (owner,
// 2026-10-03). Nothing tessellates a part for us to draw, so opening one shows its picture and
// metadata and nothing else; the model arrives only when somebody presses **view**, at which point
// the browser fetches `?variant=original` and reads it itself.
//
//   * **open** — a grid card is hovered, pressed after `--dwell` ms, and timed from `pointerdown` to
//     the quick look being on screen. No mesh is involved; this is `DATA.md` §2.5's "part open".
//   * **draw** — then **view** is pressed, and timed to the viewer's `lapidary:viewer-first-frame`
//     mark. The mark survived the pivot (R1); it times a view press rather than an open.
//
// Each draw also reports how many shader programs WebGL linked during it, counted by wrapping
// `linkProgram` before the page loads: zero means everything it drew was already compiled.
//
// With `--part`, one part, a fresh session per run. With `--direct`, each part's page is opened from
// a link in a fresh session and timed from navigation start — through the view press, since that is
// the only thing that produces a frame.
//
//   node web/scripts/open-timing.mjs [--url http://localhost:3000] [--gl swiftshader|gpu]
//     [--dwell 250] [--rounds 3] [--throttle] [--part <name> [--runs 3]] [--direct [--part <name>]]
import { spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parseArgs } from 'node:util'

const { values: args } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:3000' },
    gl: { type: 'string', default: 'swiftshader' },
    dwell: { type: 'string', default: '250' },
    rounds: { type: 'string', default: '3' },
    throttle: { type: 'boolean', default: false },
    part: { type: 'string' },
    runs: { type: 'string', default: '3' },
    direct: { type: 'boolean', default: false },
  },
})
if (!['swiftshader', 'gpu'].includes(args.gl)) throw new Error('--gl is swiftshader or gpu')
const dwell = Number(args.dwell)

// The press that loads the model, read from the strings it is rendered from — so a rename there
// fails this tool loudly instead of leaving it timing a button that is no longer there.
// Scoped to the `viewer` block: `view:` appears twice in that file, and the other one is the grid's
// own `View` menu, which would have this tool pressing a layout control and waiting for ever.
const VIEW = readFileSync(new URL('../src/lib/strings.ts', import.meta.url), 'utf8').match(
  /\n {2}viewer: \{[\s\S]*?\n {4}view: '([^']+)'/,
)?.[1]
if (VIEW === undefined) throw new Error('strings.viewer.view moved; update open-timing.mjs')

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function launch(url = args.url, ready = `document.querySelectorAll('article').length > 0`) {
  const profile = mkdtempSync(join(tmpdir(), 'lapidary-timing-'))
  const port = 9300 + Math.floor(Math.random() * 600)
  const gl =
    args.gl === 'gpu' ? ['--enable-gpu', '--ignore-gpu-blocklist'] : ['--use-angle=swiftshader', '--enable-unsafe-swiftshader']
  const chrome = spawn(
    'google-chrome',
    ['--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, '--window-size=1440,900',
      '--no-first-run', '--no-default-browser-check', ...gl, 'about:blank'],
    { stdio: 'ignore' },
  )
  const exited = new Promise((resolve) => chrome.on('exit', resolve))
  let target
  for (let i = 0; i < 100 && target === undefined; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
      target = list.find((t) => t.type === 'page')
    } catch {
      await sleep(100)
    }
  }
  if (target === undefined) throw new Error('Chrome did not open a debugging port; is google-chrome installed?')
  const ws = new WebSocket(target.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = reject
  })
  let next = 0
  const pending = new Map()
  ws.onmessage = (event) => {
    const message = JSON.parse(event.data)
    const waiting = pending.get(message.id)
    if (waiting === undefined) return
    pending.delete(message.id)
    if (message.error) waiting.reject(new Error(message.error.message))
    else waiting.resolve(message.result)
  }
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++next
      pending.set(id, { resolve, reject })
      ws.send(JSON.stringify({ id, method, params }))
    })
  const evaluate = async (expression) => {
    const { result, exceptionDetails } = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text)
    return result.value
  }
  const waitFor = async (expression, timeout = 60_000) => {
    const end = Date.now() + timeout
    while (!(await evaluate(`Boolean(${expression})`))) {
      if (Date.now() > end) throw new Error(`timed out waiting for ${expression}`)
      await sleep(20)
    }
  }
  const close = async () => {
    ws.close()
    chrome.kill()
    await exited
    rmSync(profile, { recursive: true, force: true })
  }

  await send('Page.enable')
  await send('Page.addScriptToEvaluateOnNewDocument', {
    source: `window.__links = []
      for (const context of [WebGLRenderingContext, WebGL2RenderingContext]) {
        const link = context.prototype.linkProgram
        context.prototype.linkProgram = function (program) {
          window.__links.push(performance.now())
          return link.call(this, program)
        }
      }`,
  })
  await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false })
  if (args.throttle) {
    await send('Network.enable')
    // 100 Mbit: 12.5 MB/s each way with 2 ms of latency, as the Phase 3 addendum used.
    await send('Network.emulateNetworkConditions', {
      offline: false, latency: 2, downloadThroughput: 12_500_000, uploadThroughput: 12_500_000,
    })
  }
  await send('Page.navigate', { url })
  await waitFor(ready)
  await sleep(500)
  await evaluate(`addEventListener('pointerdown', () => { window.__down = performance.now() }, { capture: true })`)
  return { send, evaluate, waitFor, close }
}

const cardName = `(article) => document.getElementById(article.getAttribute('aria-labelledby'))?.textContent.trim()`

/**
 * Hover a card, press it after the dwell, and return ms from pointerdown to the quick look.
 *
 * **No mesh is involved and that is the measurement.** The card's picture is the picture; the panel
 * carries the part's figures and its own stage, and nothing is fetched or drawn until `draw` below.
 * The point pressed is the card's well — its `<img>` when the part has a picture, the card itself
 * when it shows its format instead — and never the name, which is a link.
 */
async function open(page, name) {
  const point = await page.evaluate(`(() => {
    const article = [...document.querySelectorAll('article')].find((a) => (${cardName})(a) === ${JSON.stringify(name)})
    article.scrollIntoView({ block: 'center' })
    const box = (article.querySelector('img') ?? article).getBoundingClientRect()
    return { x: box.left + box.width / 2, y: box.top + box.height * (article.querySelector('img') ? 0.5 : 0.3) }
  })()`)
  const mouse = (type) => page.send('Input.dispatchMouseEvent', { type, ...point, button: 'left', clickCount: 1 })
  await mouse('mouseMoved')
  await sleep(dwell)
  await mouse('mousePressed')
  await mouse('mouseReleased')
  // The close control is the quick look's, at either width, and it mounts with the panel's content
  // rather than with its frame — so it is the panel being usable and not merely present.
  await page.waitFor(`document.querySelector('button[aria-label="Close"]') !== null`)
  return page.evaluate('performance.now() - window.__down')
}

/**
 * Press **view** and return ms from that press to the first frame, with the shaders it linked.
 *
 * The press is a real `Input.dispatchMouseEvent` for the same reason the card's is: a synthetic
 * `element.click()` is not a press a person could have made. The `pointerdown` listener `launch`
 * installs re-stamps `window.__down` on this press, so the number is measured from the input event
 * and not from when Node got round to asking — and `open` has already read its own.
 */
async function draw(page) {
  const point = await page.evaluate(`(() => {
    const b = [...document.querySelectorAll('button')].find((e) => e.textContent.trim() === ${JSON.stringify(VIEW)})
    if (b === undefined) return null
    b.scrollIntoView({ block: 'center' })
    const box = b.getBoundingClientRect()
    return { x: box.left + box.width / 2, y: box.top + box.height / 2 }
  })()`)
  if (point === null) throw new Error(`no button reads ${JSON.stringify(VIEW)}; this part offers no 3D view`)
  const mouse = (type) => page.send('Input.dispatchMouseEvent', { type, ...point, button: 'left', clickCount: 1 })
  await mouse('mouseMoved')
  await mouse('mousePressed')
  await mouse('mouseReleased')
  await page.waitFor(`performance.getEntriesByName('lapidary:viewer-first-frame').some((m) => m.startTime > window.__down)`)
  return page.evaluate(`(() => {
    const mark = performance.getEntriesByName('lapidary:viewer-first-frame').findLast((m) => m.startTime > window.__down)
    const linked = window.__links.filter((t) => t > window.__down && t <= mark.startTime).length
    return { ms: mark.startTime - window.__down, linked }
  })()`)
}

async function shut(page) {
  await page.evaluate(`document.querySelector('button[aria-label="Close"]').click()`)
  await page.waitFor(`document.querySelector('button[aria-label="Close"]') === null`)
  await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 2, y: 2 })
  await sleep(300)
}

function stats(values) {
  if (values.length === 0) return 'n=0'
  const sorted = [...values].sort((a, b) => a - b)
  const middle = sorted.length / 2
  const median = sorted.length % 2 ? sorted[Math.floor(middle)] : (sorted[middle - 1] + sorted[middle]) / 2
  const p90 = sorted[Math.max(0, Math.ceil(sorted.length * 0.9) - 1)]
  const f = (n) => n.toFixed(1)
  return `n=${values.length} median ${f(median)} ms, p90 ${f(p90)}, min ${f(sorted[0])}, max ${f(sorted.at(-1))}`
}

async function describe(page) {
  return page.evaluate(`(() => {
    const gl = document.createElement('canvas').getContext('webgl2')
    const info = gl?.getExtension('WEBGL_debug_renderer_info')
    return gl === null ? 'no WebGL' : gl.getParameter(info ? info.UNMASKED_RENDERER_WEBGL : gl.RENDERER)
  })()`)
}

if (args.direct) {
  // Parts by id, for their pages' URLs, from the library the grid opens on.
  const library = readFileSync(new URL('../src/lib/api.ts', import.meta.url), 'utf8').match(
    /DEFAULT_LIBRARY_ID: LibraryId = '([^']+)'/,
  )?.[1]
  if (library === undefined) throw new Error('DEFAULT_LIBRARY_ID moved; update open-timing.mjs')
  const { parts } = await (await fetch(`${args.url}/api/libraries/${library}/parts?limit=500`)).json()
  const chosen = parts.filter((part) => args.part === undefined || part.name === args.part)
  const draws = []
  for (let round = 0; round < Number(args.rounds); round++) {
    for (const part of chosen) {
      // Ready is the **view** button, not a first frame: a part page draws nothing on its own now, so
      // waiting for a mark here would wait for ever. The press is what produces one.
      const page = await launch(
        `${args.url}/parts/${part.id}`,
        `[...document.querySelectorAll('button')].some((e) => e.textContent.trim() === ${JSON.stringify(VIEW)})`,
      )
      try {
        if (draws.length === 0) console.log(`${await describe(page)} | from a link, a fresh session each open, then one view press | ${args.throttle ? '100 Mbit' : 'local'} | ${chosen.length} parts`)
        draws.push(await draw(page))
      } finally {
        await page.close()
      }
    }
  }
  console.log(`  view press to first frame, cold page: ${stats(draws.map((d) => d.ms))}`)
  console.log(`  per draw: ${draws.map((d) => Math.round(d.ms)).join(' ')}`)
  console.log(`  shaders linked during each: ${draws.map((d) => d.linked).join(' ')}`)
} else if (args.part === undefined) {
  const page = await launch()
  try {
    const names = await page.evaluate(`[...document.querySelectorAll('article')].map(${cardName})`)
    console.log(`${await describe(page)} | dwell ${dwell} ms | ${args.throttle ? '100 Mbit' : 'local'} | ${names.length} parts`)
    const rows = []
    for (let round = 0; round < Number(args.rounds); round++) {
      for (const name of names) {
        const ms = await open(page, name)
        // Not every part offers one: a B-rep part ingested before the pivot keeps its figures and its
        // download and has no **view** at all, which is correct and not a missing measurement.
        let drawn = null
        try {
          drawn = await draw(page)
        } catch {
          /* no view button on this part */
        }
        rows.push({ round, ms, draw: drawn })
        await shut(page)
      }
    }
    const drawn = rows.filter((r) => r.draw !== null)
    const [first, ...rest] = rows
    console.log(`  first open in the session: ${first.ms.toFixed(1)} ms`)
    console.log(`  round 0, other parts' first opens: ${stats(rest.filter((o) => o.round === 0).map((o) => o.ms))}`)
    console.log(`  later rounds, parts opened before: ${stats(rest.filter((o) => o.round > 0).map((o) => o.ms))}`)
    console.log(`  per open: ${rows.map((o) => Math.round(o.ms)).join(' ')}`)
    console.log(`  view press to first frame: ${stats(drawn.map((o) => o.draw.ms))} (${drawn.length} of ${rows.length} parts offer a view)`)
    console.log(`  shaders linked during each draw: ${drawn.map((o) => o.draw.linked).join(' ')}`)
  } finally {
    await page.close()
  }
} else {
  const opens = []
  const draws = []
  for (let run = 0; run < Number(args.runs); run++) {
    const page = await launch()
    try {
      if (run === 0) console.log(`${await describe(page)} | dwell ${dwell} ms | ${args.throttle ? '100 Mbit' : 'local'} | ${args.part}`)
      opens.push(await open(page, args.part))
      draws.push((await draw(page)).ms)
    } finally {
      await page.close()
    }
  }
  console.log(`  open, a fresh session each run: ${stats(opens)}`)
  console.log(`  view press to first frame: ${stats(draws)}`)
}
