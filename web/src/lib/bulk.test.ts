import { expect, test } from 'vitest'
import { eachAtMost } from './bulk'

test('runs at most the limit at once and reports failures in the order given', async () => {
  let running = 0
  let most = 0
  const failures = await eachAtMost([1, 2, 3, 4, 5, 6, 7, 8, 9], 4, async (n) => {
    running += 1
    most = Math.max(most, running)
    // Later items finish first, so a result collected in completion order would come back
    // reversed.
    await new Promise((resolve) => setTimeout(resolve, 20 - n))
    running -= 1
    return n % 3 === 0 ? `part ${n} was not changed` : null
  })

  expect(most).toBe(4)
  expect(failures).toEqual([
    { item: 3, reason: 'part 3 was not changed' },
    { item: 6, reason: 'part 6 was not changed' },
    { item: 9, reason: 'part 9 was not changed' },
  ])
})
