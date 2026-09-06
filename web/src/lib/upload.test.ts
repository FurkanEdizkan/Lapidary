// @vitest-environment node
import { afterEach, expect, test, vi } from 'vitest'
import { DEFAULT_LIBRARY_ID } from './api'
import { filesFromDrop, hashFile, uploadFiles } from './upload'
import type { PickedFile } from './upload'

/**
 * The transfer engine, which is where the sequencing lives.
 *
 * Node, not jsdom: jsdom's `File` has no `.stream()`, and streaming the file rather than
 * reading it whole is the property under test — a polyfill that buffered it would make
 * every assertion here pass over the bug it exists to prevent. Nothing in this file
 * touches the DOM.
 *
 * `crates/lapidary-api/tests/upload.rs` drives each route directly and proves what each
 * one promises. None of that says the client calls them in the right order, sends only
 * what the probe said to send, or commits once — and those are the three things that turn
 * a working route into a working upload. They are also invisible from the server side: a
 * client that uploads every file anyway and commits them one at a time produces a correct
 * library, slowly, with 500 progress bars.
 */

afterEach(() => {
  vi.unstubAllGlobals()
})

const LIBRARY = DEFAULT_LIBRARY_ID

function pick(path: string, body: string): PickedFile {
  return { file: new File([body], path.split('/').pop() ?? path), path }
}

interface Call {
  url: string
  method: string
  body: unknown
}

/**
 * A fetch that records every call and answers from a probe plan. Chunks answer with the
 * running length of what they were sent, which is what the real route answers.
 */
function stubApi(plan: { have?: string[]; needRows?: string[]; needBytes?: string[] }) {
  const calls: Call[] = []
  const received = new Map<string, number>()
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET'
      if (url.includes('/uploads/probe')) {
        calls.push({ url, method, body: JSON.parse(String(init?.body)) })
        return new Response(
          JSON.stringify({
            have: plan.have ?? [],
            needRows: plan.needRows ?? [],
            needBytes: plan.needBytes ?? [],
          }),
        )
      }
      if (url.includes('/uploads/commit')) {
        const body = JSON.parse(String(init?.body)) as { files: unknown[] }
        calls.push({ url, method, body })
        return new Response(
          JSON.stringify({ batchId: '01931b6e-0000-7000-8000-0000000000ff', queued: body.files.length }),
        )
      }
      // A chunk. The hash is the last path segment before the query.
      const hash = url.split('/uploads/')[1]!.split('?')[0]!
      const size = (init?.body as Blob).size
      calls.push({ url, method, body: size })
      const next = (received.get(hash) ?? 0) + size
      received.set(hash, next)
      return new Response(JSON.stringify({ received: next }))
    }),
  )
  return calls
}

test('a file is hashed the way the server hashes it', async () => {
  // The one number both sides have to agree on, and the reason the whole probe works.
  // Pinned against the digest `blake3::hash` produces for the same bytes, taken from the
  // Rust side rather than from this implementation.
  const hash = await hashFile(new File(['the quick brown fox jumps over the lazy dog'], 'x.stl'))
  expect(hash).toBe('54eb9a529fa1343a5b1e16eaade0c274d0b58e9d7a02b017f8fc01cbe0e2dfde')
})

test('only the files the probe asked for are transferred, and all of them are committed', async () => {
  // The branch the probe exists for. `needRows` bytes are already in the store, so they
  // must skip the transfer entirely and still reach the manifest — a client that uploads
  // everything anyway is correct and moves 25 GB to do it, and one that commits only what
  // it transferred silently drops every file it deduplicated.
  const picked = [
    pick('brackets/a.stl', 'aaaa'),
    pick('brackets/b.stl', 'bbbb'),
    pick('brackets/c.stl', 'cccc'),
  ]
  const calls = stubApi({
    have: ['brackets/a.stl'],
    needRows: ['brackets/b.stl'],
    needBytes: ['brackets/c.stl'],
  })

  const result = await uploadFiles(LIBRARY, picked, () => {})

  const chunks = calls.filter((call) => call.method === 'PUT')
  expect(chunks).toHaveLength(1)
  expect(chunks[0]!.url).toContain(await hashFile(picked[2]!.file))

  const commits = calls.filter((call) => call.url.includes('/uploads/commit'))
  expect(commits).toHaveLength(1)
  const manifest = (commits[0]!.body as { files: { path: string }[] }).files
  expect(manifest.map((file) => file.path)).toEqual(['brackets/b.stl', 'brackets/c.stl'])
  expect(result.alreadyHere).toBe(1)
  expect(result.bytesSkipped).toBe(4)
})

test('a drop of many files is committed in one call, not one per file', async () => {
  // One batch for the whole drop. Committing per file would give a folder of 500 parts
  // 500 batches, which is 500 progress bars.
  const picked = Array.from({ length: 12 }, (_, i) => pick(`parts/p${i}.stl`, `body ${i}`))
  const calls = stubApi({ needBytes: picked.map((p) => p.path) })

  await uploadFiles(LIBRARY, picked, () => {})

  expect(calls.filter((call) => call.url.includes('/uploads/commit'))).toHaveLength(1)
  expect(calls.filter((call) => call.url.includes('/uploads/probe'))).toHaveLength(1)
})

test('a chunk the server already holds resumes from where the server is', async () => {
  // The 409 path. A client that treated it as a failure would restart a 2 GB transfer
  // from zero; one that ignored the answer would append the same bytes twice and produce
  // a file that can never verify.
  const picked = [pick('a.stl', 'x'.repeat(64))]
  const hash = await hashFile(picked[0]!.file)
  const offsets: number[] = []
  let firstChunk = true
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes('/uploads/probe')) {
        return new Response(JSON.stringify({ have: [], needRows: [], needBytes: ['a.stl'] }))
      }
      if (url.includes('/uploads/commit')) {
        return new Response(JSON.stringify({ batchId: 'b', queued: 1 }))
      }
      offsets.push(Number(new URL(url, 'http://x').searchParams.get('offset')))
      if (firstChunk) {
        // The server already holds 40 bytes of this file from an interrupted attempt.
        firstChunk = false
        return new Response(JSON.stringify({ received: 40 }), { status: 409 })
      }
      return new Response(JSON.stringify({ received: 64 }))
    }),
  )

  await uploadFiles(LIBRARY, picked, () => {})

  expect(offsets).toEqual([0, 40])
  expect(hash).toHaveLength(64)
})

test('a folder drop reads past the hundredth entry', async () => {
  // `readEntries` returns at most 100 entries per call and signals the end with an empty
  // array; it does not return the whole directory. Calling it once reads the first 100
  // files of a parts library and silently drops the rest, which is most of it — the exact
  // failure this loop exists for, and one no amount of manual testing on a small folder
  // would show.
  const names = Array.from({ length: 250 }, (_, i) => `p${i}.stl`)
  let cursor = 0
  const entries = names.map((name) => ({
    isFile: true,
    isDirectory: false,
    fullPath: `/parts/${name}`,
    file: (resolve: (f: File) => void) => resolve(new File(['x'], name)),
  }))
  const folder = {
    isFile: false,
    isDirectory: true,
    fullPath: '/parts',
    createReader: () => ({
      readEntries: (resolve: (batch: unknown[]) => void) => {
        const batch = entries.slice(cursor, cursor + 100)
        cursor += batch.length
        resolve(batch)
      },
    }),
  }
  const items = [{ webkitGetAsEntry: () => folder }] as unknown as DataTransferItemList

  const picked = await filesFromDrop(items)

  expect(picked).toHaveLength(250)
  // And the leading slash is gone: `fullPath` is absolute within the drop, and the server
  // refuses an absolute `source_path` outright.
  expect(picked[0]!.path).toBe('parts/p0.stl')
  expect(picked[249]!.path).toBe('parts/p249.stl')
})

test('a re-drop of a folder this library already holds sends and commits nothing', async () => {
  // The path `strings.upload.nothingToDo` exists for, and the one a user hits by dropping
  // the same folder twice. Nothing is transferred and the manifest is empty, which the
  // route answers `202 { queued: 0 }` to — a success with no batch to poll. A client that
  // threw here, or a route that refused an empty manifest, would turn the most ordinary
  // repeat gesture in the app into a failure banner.
  const picked = [pick('brackets/a.stl', 'aaaa'), pick('brackets/b.stl', 'bbbb')]
  const calls = stubApi({ have: picked.map((p) => p.path) })

  const result = await uploadFiles(LIBRARY, picked, () => {})

  expect(calls.filter((call) => call.method === 'PUT')).toHaveLength(0)
  const commits = calls.filter((call) => call.url.includes('/uploads/commit'))
  expect(commits).toHaveLength(1)
  expect((commits[0]!.body as { files: unknown[] }).files).toEqual([])
  expect(result.accepted.queued).toBe(0)
  expect(result.alreadyHere).toBe(2)
})
