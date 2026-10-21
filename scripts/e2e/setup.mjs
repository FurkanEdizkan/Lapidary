#!/usr/bin/env node
// First-run setup through the wizard itself (A41), in a real browser, on a stack nobody has set up yet.
//
//   node scripts/e2e/setup.mjs --web <url> --code <setup code> --user <name> --password <password> --out <dir>
//
// `stack.sh` runs it from `sign_in_rig` when `/api/session` answers 409, so every fresh rig stack is set up the way
// a person does it: the intro and its Skip, a wrong code sent back to step 1, the steps forward and back, the
// Options step's sign-in pictures and import count, and Finish. The import count is changed on purpose: Finish then
// restarts Lapidary, and the script follows it back and reads the number the worker took.
//
// The words are `web/src/lib/strings.ts`'s, copied as `flows.mjs`'s `T` copies them.
import { mkdirSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { session } from './cdp.mjs'

const { values: args } = parseArgs({
  options: {
    web: { type: 'string' },
    code: { type: 'string' },
    user: { type: 'string' },
    password: { type: 'string' },
    out: { type: 'string' },
  },
})
for (const required of ['web', 'code', 'user', 'password', 'out']) {
  if (args[required] === undefined) throw new Error(`--${required} is required`)
}

const T = {
  skip: 'Skip',
  code: 'Setup code',
  username: 'Username',
  password: 'Password',
  confirm: 'Password, again',
  next: 'Next',
  back: 'Back',
  finish: 'Finish setup',
  wrongCode: 'That setup code is not the one this server printed.',
  wall: 'Show pictures on the sign-in page',
  workers: 'Imports at once',
}
const WORKERS = 3

function expect(ok, message) {
  if (!ok) throw new Error(message)
}

mkdirSync(args.out, { recursive: true })
const page = await session({ width: 1440, height: 900, base: args.web, shots: args.out })
const done = []
try {
  // The input inside the label whose own first words are `label`: a label holds its hint as well.
  const field = (label) =>
    `[...document.querySelectorAll('label')].find((l) => l.firstChild?.textContent?.trim() === ${JSON.stringify(label)})?.querySelector('input')`
  const fill = async (label, value) => {
    const set = await page.evaluate(`(() => {
      const input = ${field(label)}
      if (!input) return false
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(String(value))})
      input.dispatchEvent(new Event('input', { bubbles: true }))
      return true
    })()`)
    expect(set, `no field labelled "${label}"`)
  }
  const press = async (text) => {
    const button = await page.byText('button', text, { exact: true })
    expect(button !== null, `no "${text}" button`)
    await page.click(button.x, button.y)
  }
  const at = (name) => `document.querySelector('[aria-current="step"]')?.textContent?.trim() === ${JSON.stringify(name)}`

  // The intro plays once, and Skip ends it.
  await page.go('/setup', `[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === ${JSON.stringify(T.skip)})`)
  await press(T.skip)
  await page.waitFor(`Boolean(${field(T.code)})`, 10_000)
  done.push('intro skipped')

  // Step 1 with a wrong code: the code is only checked at Finish, so it comes back here.
  await fill(T.code, 'WRNG-CODE-0000')
  await fill(T.username, args.user)
  await fill(T.password, args.password)
  await fill(T.confirm, args.password)
  await press(T.next)
  await page.waitFor(at('Server'), 5_000)
  // Back keeps what was typed.
  await press(T.back)
  await page.waitFor(at('Account'), 5_000)
  const kept = await page.evaluate(`${field(T.username)}?.value`)
  expect(kept === args.user, `Back lost the username: "${kept}"`)
  await press(T.next)
  await page.waitFor(at('Server'), 5_000)
  await press(T.next)
  await page.waitFor(at('Options'), 5_000)
  done.push('steps forward, back and forward')

  // Options: the sign-in pictures on by default, and the import count once the server has said it.
  const wall = await page.evaluate(
    `[...document.querySelectorAll('label')].find((l) => l.textContent.includes(${JSON.stringify(T.wall)}))?.querySelector('input')?.checked`,
  )
  expect(wall === true, `the sign-in pictures switch reads ${wall}, not on`)
  // Without a valid code the server will not say its count, so the field waits; it is the code that is wrong.
  await press(T.finish)
  await page.waitFor(`${at('Account')} && document.body.textContent.includes(${JSON.stringify(T.wrongCode)})`, 10_000)
  done.push('a wrong code sent back to step 1')

  await fill(T.code, args.code)
  await press(T.next)
  await page.waitFor(at('Server'), 5_000)
  await press(T.next)
  await page.waitFor(at('Options'), 5_000)
  await page.waitFor(`(() => { const i = ${field(T.workers)}; return i && !i.disabled && i.value !== '' })()`, 10_000)
  const before = Number(await page.evaluate(`${field(T.workers)}.value`))
  expect(before !== WORKERS, `the import count already reads ${WORKERS}; the restart would not be tested`)
  await fill(T.workers, WORKERS)
  await page.shot('setup-options')
  await press(T.finish)

  // Finish restarts on the new count; the page follows it back and lands signed in.
  await page.waitFor(`location.pathname === '/' && document.querySelector('button[popovertarget="user-menu"]') !== null`, 180_000)
  const workers = await page.evaluate(`fetch('/api/settings/workers').then((r) => r.json())`)
  expect(workers.workers === WORKERS && workers.lockedBy === null, `after Finish the import count reads ${JSON.stringify(workers)}`)
  const pictures = await page.evaluate(`fetch('/api/settings/wall').then((r) => r.json())`)
  expect(pictures.on === true, `after Finish the sign-in pictures read ${JSON.stringify(pictures)}`)
  done.push(`finished, restarted with ${before} → ${WORKERS} imports at once, signed in`)
  await page.shot('setup-done')
  console.log(`  ok   setup  ${done.join('; ')}`)
} catch (error) {
  try {
    await page.shot('setup-failed')
  } catch {
    // The failure is what matters.
  }
  console.log(`FAILED setup  ${error.message}  (after: ${done.join('; ') || 'nothing'})`)
  process.exitCode = 1
} finally {
  await page.close()
}
