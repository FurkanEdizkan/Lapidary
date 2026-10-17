import { Unviewable, meshPositions } from './mesh'

/**
 * `loadMesh`'s worker (A11): the original file in, its triangle positions out, both moved rather than
 * copied, so reading a large model never holds up the page.
 */
self.onmessage = (event: MessageEvent<{ format: string; bytes: ArrayBuffer }>) => {
  try {
    const positions = meshPositions(event.data.format, event.data.bytes)
    self.postMessage({ positions }, { transfer: [positions.buffer] })
  } catch (error) {
    self.postMessage(
      error instanceof Unviewable
        ? { unviewable: error.message }
        : { failed: error instanceof Error ? error.message : String(error) },
    )
  }
}
