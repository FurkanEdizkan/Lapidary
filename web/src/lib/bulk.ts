/**
 * Runs `task` over `items`, at most `limit` at a time, and returns the items it reported a
 * reason for, in the order they were given.
 *
 * Per-part requests rather than a batch route, until measurement shows the loop is too slow.
 * A small limit keeps a selection of hundreds from opening hundreds of connections at once
 * while a few dozen still finish in about the time a person takes to look back at the grid.
 *
 * `task` reports a failure by returning the sentence a person reads, and does not throw: a
 * throw would end its worker early and leave the rest of that worker's parts untried.
 */
export async function eachAtMost<T>(
  items: readonly T[],
  limit: number,
  task: (item: T) => Promise<string | null>,
): Promise<{ item: T; reason: string }[]> {
  const reasons: (string | null)[] = items.map(() => null)
  let next = 0
  const worker = async () => {
    while (next < items.length) {
      const index = next++
      reasons[index] = await task(items[index] as T)
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker))
  return items.flatMap((item, index) => {
    const reason = reasons[index]
    return reason == null ? [] : [{ item, reason }]
  })
}
