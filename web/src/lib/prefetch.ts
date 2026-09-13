/**
 * Warms the browser's HTTP cache for blobs a person is likely to open next (`DATA.md` §2.4).
 *
 * Blob responses are `immutable`, so fetching one and dropping the body is enough: the viewer's own
 * request for the same URL is answered from cache. At most `limit` at once, each URL once, and
 * `cancel` aborts what is in flight and forgets what was queued, so scrolling fast past a hundred
 * cards does not leave a hundred requests behind it.
 */
export function createPrefetch(
  limit: number,
  fetcher: (url: string, init: { signal: AbortSignal }) => Promise<unknown> = (url, init) =>
    fetch(url, init).then((response) => response.arrayBuffer()),
) {
  const seen = new Set<string>()
  const queue: string[] = []
  let running = 0
  let controller = new AbortController()
  const pump = () => {
    while (running < limit && queue.length > 0) {
      const url = queue.shift()
      if (url === undefined) return
      running += 1
      void fetcher(url, { signal: controller.signal })
        // A prefetch that fails costs nothing: the real request, if it comes, says why.
        .catch(() => undefined)
        .finally(() => {
          running -= 1
          pump()
        })
    }
  }
  return {
    request(url: string) {
      if (seen.has(url)) return
      seen.add(url)
      queue.push(url)
      pump()
    },
    cancel() {
      controller.abort()
      controller = new AbortController()
      queue.length = 0
      seen.clear()
    },
  }
}
