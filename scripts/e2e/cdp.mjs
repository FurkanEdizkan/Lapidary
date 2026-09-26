// One headless Chrome session over the DevTools protocol, with nothing but Node's built-ins.
//
// The transport is `web/scripts/open-timing.mjs`'s, and that is not a preference. Two halves of it are
// load-bearing and `target/ui-verify/verify.mjs`'s equivalents are wrong in ways an assertion cannot
// see:
//
//   * `send` **rejects** on `message.error`. verify.mjs resolves every message, so a CDP method that
//     failed reads as a method that worked.
//   * `evaluate` **throws** on `exceptionDetails`. verify.mjs returns `undefined` instead, so
//     `expect(await page.evaluate('...'))` would pass silently on a page that threw — which for a
//     harness whose whole job is asserting is the one failure mode that must not exist.
//   * the debugging port is **random**. verify.mjs pins 9400, and two lanes driving at once collide.
//   * the profile directory is **removed on close**. verify.mjs leaks one per session into `out`.
//
// The helpers are verify.mjs's, lifted as they are because they were tuned against this application:
// `shot`, `go`, `mouse`, `click`, `key`, `rect`, the device-metrics override, `setEmulatedMedia` for
// reduced motion, and the `matchMedia('(hover: hover)')` shim — headless Chrome reports a screen that
// cannot hover, and a good deal of the grid hangs off that query, so without the shim the cards under
// test are not the cards anybody uses.
import { spawn } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/**
 * A browser session at one viewport.
 *
 * `hover` shims `(hover: hover)`; `reducedMotion` emulates `prefers-reduced-motion: reduce`; `shots` is
 * where `shot(name)` writes. `close()` kills Chrome and removes its profile.
 */
export async function session({
  width = 1440,
  height = 900,
  base,
  shots,
  reducedMotion = false,
  hover = true,
  gl = 'swiftshader',
} = {}) {
  if (base === undefined) throw new Error('session({ base }) is the url the stack answers on')
  const profile = mkdtempSync(join(tmpdir(), 'lapidary-e2e-'))
  // Random, in a range no other harness here pins: verify.mjs's fixed 9400 collided between lanes.
  const port = 9300 + Math.floor(Math.random() * 600)
  const glFlags =
    gl === 'gpu'
      ? ['--enable-gpu', '--ignore-gpu-blocklist']
      : ['--use-angle=swiftshader', '--enable-unsafe-swiftshader']
  const chrome = spawn(
    'google-chrome',
    [
      '--headless=new',
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      `--window-size=${width},${height}`,
      '--hide-scrollbars',
      '--no-first-run',
      '--no-default-browser-check',
      '--enable-precise-memory-info',
      ...glFlags,
      'about:blank',
    ],
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
  if (target === undefined) {
    chrome.kill()
    rmSync(profile, { recursive: true, force: true })
    throw new Error('Chrome did not open a debugging port; is google-chrome installed?')
  }

  const ws = new WebSocket(target.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = reject
  })
  let next = 0
  const pending = new Map()
  const events = new Map()
  ws.onmessage = (event) => {
    const message = JSON.parse(event.data)
    if (message.id === undefined) {
      events.get(message.method)?.push(message.params)
      return
    }
    const waiting = pending.get(message.id)
    if (waiting === undefined) return
    pending.delete(message.id)
    // Rejects. A CDP call that failed must not read as one that worked.
    if (message.error) waiting.reject(new Error(`${message.method ?? 'CDP'}: ${message.error.message}`))
    else waiting.resolve(message.result)
  }
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++next
      pending.set(id, { resolve, reject, method })
      ws.send(JSON.stringify({ id, method, params }))
    })

  // Throws. An assertion written against this cannot pass on a page that threw.
  const evaluate = async (expression) => {
    const { result, exceptionDetails } = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    })
    if (exceptionDetails) {
      throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text)
    }
    return result.value
  }

  const waitFor = async (expression, timeout = 30_000) => {
    const end = Date.now() + timeout
    while (!(await evaluate(`Boolean(${expression})`))) {
      if (Date.now() > end) throw new Error(`timed out after ${timeout} ms waiting for ${expression}`)
      await sleep(50)
    }
    return true
  }

  /**
   * Read a measurement until it settles, then return it.
   *
   * The grid renders thumbnails lazily and the viewer swaps meshes in, so the first answer to "how
   * many cards are there" is routinely not the last. This is verify.mjs's retry loop, made explicit:
   * `read` is called until `accept` likes the answer, and the last answer is returned either way so a
   * failure reports what it actually saw.
   */
  const settle = async (read, accept, { timeout = 15_000, every = 250 } = {}) => {
    const end = Date.now() + timeout
    let last
    for (;;) {
      last = await read()
      if (accept(last)) return last
      if (Date.now() > end) return last
      await sleep(every)
    }
  }

  const shot = async (name) => {
    if (shots === undefined) throw new Error('session({ shots }) is where screenshots go')
    const { data } = await send('Page.captureScreenshot')
    const path = join(shots, `${name}.png`)
    writeFileSync(path, Buffer.from(data, 'base64'))
    return path
  }

  const go = async (path, ready) => {
    await send('Page.navigate', { url: path.startsWith('http') ? path : base + path })
    // A settled DOM, not a fixed sleep, whenever the caller can name what it is waiting for.
    if (ready) await waitFor(ready)
    else await sleep(1200)
  }

  const mouse = (x, y, type = 'mouseMoved') =>
    send('Input.dispatchMouseEvent', {
      type,
      x: Math.round(x),
      y: Math.round(y),
      button: type === 'mouseMoved' ? 'none' : 'left',
      clickCount: type === 'mouseMoved' ? 0 : 1,
    })

  const click = async (x, y) => {
    await mouse(x, y, 'mousePressed')
    await mouse(x, y, 'mouseReleased')
  }

  const key = async (k, code, keyCode, text) => {
    await send('Input.dispatchKeyEvent', {
      type: text === undefined ? 'rawKeyDown' : 'keyDown',
      key: k,
      code,
      windowsVirtualKeyCode: keyCode,
      ...(text === undefined ? {} : { text }),
    })
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: keyCode })
  }

  const type = async (text) => {
    for (const ch of text) await send('Input.insertText', { text: ch })
  }

  /** The centre of the `index`th match, in viewport coordinates, or null when there is none. */
  const rect = (selector, index = 0) =>
    evaluate(`(() => {
      const e = document.querySelectorAll(${JSON.stringify(selector)})[${index}]
      if (!e) return null
      e.scrollIntoView({ block: 'center' })
      const r = e.getBoundingClientRect()
      return { x: r.x + r.width / 2, y: r.y + r.height / 2, width: r.width, height: r.height, top: r.top, left: r.left }
    })()`)

  /**
   * Set a React-controlled `<input>` or `<select>`.
   *
   * `element.value = x` is reverted on the next render — React keeps the value and only the setter on
   * the prototype tells it otherwise. So: call the native setter, then dispatch the event React
   * listens for. This is the only way to move a `<select>` in headless, where a synthetic click opens
   * no popup at all.
   */
  const setControl = (selector, value, event = 'change') =>
    evaluate(`(() => {
      const e = document.querySelector(${JSON.stringify(selector)})
      if (!e) return null
      const proto = e.tagName === 'SELECT' ? HTMLSelectElement.prototype : e.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
      Object.getOwnPropertyDescriptor(proto, 'value').set.call(e, ${JSON.stringify(String(value))})
      e.dispatchEvent(new Event(${JSON.stringify(event)}, { bubbles: true }))
      return e.value
    })()`)

  /**
   * Open one of the three popover menus.
   *
   * Their children are in the document at all times but `display: none` until the popover opens, so a
   * driver reading `value` can skip this and a driver clicking by coordinate cannot.
   */
  const openMenu = async (id) => {
    const opened = await evaluate(`(() => {
      const trigger = document.querySelector('button[popovertarget=${JSON.stringify(id).replaceAll('"', "'")}]')
      if (!trigger) return false
      trigger.click()
      return true
    })()`)
    if (opened) await sleep(250)
    return opened
  }

  /** The centre of the first element whose accessible text matches, searched by role or tag. */
  const byText = (selector, text, { exact = false } = {}) =>
    evaluate(`(() => {
      const want = ${JSON.stringify(text)}
      const all = [...document.querySelectorAll(${JSON.stringify(selector)})]
      const e = all.find((n) => {
        const t = (n.textContent ?? '').trim()
        return ${exact ? 't === want' : 't.includes(want)'}
      })
      if (!e) return null
      e.scrollIntoView({ block: 'center' })
      const r = e.getBoundingClientRect()
      return { x: r.x + r.width / 2, y: r.y + r.height / 2, width: r.width, height: r.height }
    })()`)

  /** How far the JS heap moves across `times` hovers over the cards — verify.mjs's leak check. */
  const heapAcrossHovers = async (times = 50, cardSelector = 'article') => {
    await send('HeapProfiler.enable')
    await send('HeapProfiler.collectGarbage')
    const before = await evaluate('performance.memory.usedJSHeapSize')
    const cards = await evaluate(`document.querySelectorAll(${JSON.stringify(cardSelector)}).length`)
    if (cards === 0) return { beforeKB: Math.round(before / 1024), afterKB: null, cards: 0 }
    for (let i = 0; i < times; i++) {
      const card = await rect(cardSelector, i % cards)
      if (card === null) break
      await mouse(card.x, card.y - 30)
      await sleep(220)
      await mouse(5, height - 5)
      await sleep(40)
    }
    await sleep(600)
    await send('HeapProfiler.collectGarbage')
    const after = await evaluate('performance.memory.usedJSHeapSize')
    return { beforeKB: Math.round(before / 1024), afterKB: Math.round(after / 1024), cards, hovers: times }
  }

  /** Collect one CDP domain's events into an array the caller can read after the fact. */
  const collect = (method) => {
    const seen = []
    events.set(method, seen)
    return seen
  }

  const close = async () => {
    try {
      ws.close()
    } catch {
      /* already gone */
    }
    chrome.kill()
    await exited
    rmSync(profile, { recursive: true, force: true })
  }

  await send('Page.enable')
  await send('Runtime.enable')
  await send('Emulation.setDeviceMetricsOverride', {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 700,
  })
  // One call, because `setEmulatedMedia` replaces the whole feature list rather than adding to it.
  //
  // `hover`/`any-hover`/`pointer` are the CSS half of the hover problem and the shim below is the JS
  // half: Tailwind wraps every `hover:` utility in `@media (hover: hover)`, and a `matchMedia` shim
  // cannot reach a media query the stylesheet asks. Without these, a row's own actions — Rename,
  // Delete, Share in the category tree — stay `opacity-0 pointer-events-none` and a click by
  // coordinate silently lands on whatever is underneath.
  const features = []
  if (reducedMotion) features.push({ name: 'prefers-reduced-motion', value: 'reduce' })
  if (hover) {
    features.push({ name: 'hover', value: 'hover' }, { name: 'any-hover', value: 'hover' }, { name: 'pointer', value: 'fine' })
  }
  if (features.length > 0) await send('Emulation.setEmulatedMedia', { features })
  if (hover) {
    // The JS half. `Card.tsx`'s turntable asks `matchMedia('(hover: hover)')` directly and declines a
    // pointer-triggered spin outright when it is false, so without this the cards under test are not
    // the cards anybody uses. Installed before any document runs, so the first render already sees it.
    await send('Page.addScriptToEvaluateOnNewDocument', {
      source: `const real = window.matchMedia.bind(window)
        window.matchMedia = (q) =>
          q === '(hover: hover)'
            ? { matches: true, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false } }
            : real(q)`,
    })
  }

  // The grid pages on an `IntersectionObserver`, and an observer never fires in a tab that is not the
  // frontmost one. A driver that skips this sees a grid that will not page and a lazy thumbnail that
  // never loads, and both look like application bugs.
  await send('Page.bringToFront')

  return {
    width,
    height,
    base,
    send,
    evaluate,
    waitFor,
    settle,
    shot,
    go,
    mouse,
    click,
    key,
    type,
    rect,
    byText,
    setControl,
    openMenu,
    heapAcrossHovers,
    collect,
    close,
  }
}

/**
 * Give the file input behind a control real files. `DOM.setFileInputFiles` is the only way: a file
 * input's value cannot be set from script, so an upload driven any other way is not the upload the
 * person does.
 */
export async function setFiles(page, selector, files) {
  const { root } = await page.send('DOM.getDocument', { depth: -1 })
  const { nodeId } = await page.send('DOM.querySelector', { nodeId: root.nodeId, selector })
  if (!nodeId) throw new Error(`no file input matched ${selector}`)
  await page.send('DOM.setFileInputFiles', { files, nodeId })
}
