import { expect, test } from 'vitest'
import { createPrefetch } from './prefetch'

test('at most the limit at once, each URL once, and cancel aborts and forgets', async () => {
  const calls: { url: string; signal: AbortSignal; done: () => void }[] = []
  const prefetch = createPrefetch(2, (url, { signal }) => {
    return new Promise<void>((resolve) => calls.push({ url, signal, done: resolve }))
  })

  for (const url of ['/api/blob/a', '/api/blob/b', '/api/blob/c', '/api/blob/a', '/api/blob/d']) {
    prefetch.request(url)
  }
  expect(calls.map(({ url }) => url)).toEqual(['/api/blob/a', '/api/blob/b'])

  calls[0]?.done()
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(calls.map(({ url }) => url)).toEqual(['/api/blob/a', '/api/blob/b', '/api/blob/c'])

  prefetch.cancel()
  expect(calls[1]?.signal.aborted).toBe(true)
  expect(calls[2]?.signal.aborted).toBe(true)
  calls[1]?.done()
  calls[2]?.done()
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(calls).toHaveLength(3)
})
