import { createBLAKE3 } from 'hash-wasm'
import { commitUpload, probeUpload, putChunk } from './api'
import type { BlobHash, LibraryId, ScanAccepted, UploadFile } from './types'

/**
 * Putting a folder of files into a library from the browser: hash, probe, transfer,
 * commit.
 *
 * The order is the design, and it is the server's order read backwards. `DATA.md` §5.2
 * requires the hash before the transfer, because the hash is what lets the probe answer
 * "we already have this" — and the same hash is then the dedup key the server re-verifies
 * against, so a wrong one is not a slow upload but a corrupted one.
 *
 * # Why the hash is computed here and not on the server
 *
 * Because a hash computed on the server has already cost the transfer. Re-importing the
 * owner's 25.7 GB corpus into a library that holds it moves 25.7 GB if the client cannot
 * say what it is about to send, and moves almost nothing if it can.
 *
 * BLAKE3 has no `crypto.subtle` implementation, so this is the one place a dependency was
 * unavoidable. `hash-wasm` carries its wasm inlined as base64 rather than fetching a
 * `.wasm` asset at runtime, which is what makes it work behind `deploy/web/Caddyfile` and
 * in an air-gapped install with no second request to serve. The alternative — compiling
 * the workspace's own `blake3` crate with `wasm-pack` — would put a wasm toolchain into a
 * container build for the same bytes.
 */

/**
 * 8 MiB. The server refuses a chunk over 16 MiB, so this leaves room and still makes a
 * 2 GB file 256 requests rather than 2,000. Smaller would give finer progress and more
 * round trips; this is the size at which a chunk lost to a dropped connection costs a
 * second of transfer rather than a minute.
 */
const CHUNK_BYTES = 8 * 1024 * 1024

/** What the caller is told while this runs. Every field is a count, never a percentage:
 *  a percentage of a 25 GB drop rounds to the same number for minutes at a time. */
export interface UploadProgress {
  phase: 'hashing' | 'probing' | 'transferring' | 'committing'
  /** Files hashed, or files transferred, depending on the phase. */
  filesDone: number
  filesTotal: number
  /** Bytes actually put on the wire, and how many there are to put. Both are zero until
   *  the probe has decided what is actually being sent. */
  bytesSent: number
  bytesToSend: number
  /** Already indexed at this path with these bytes: nothing to send and nothing to do. */
  alreadyHere: number
  /** In the store already, so only the rows are missing — the probe's larger win. */
  bytesSkipped: number
}

/**
 * What the drop came to, once it is queued.
 *
 * The two savings figures ride back with the batch rather than being read off the last
 * progress tick, because the caller has no reason to keep that tick and would otherwise
 * report the number it happened to still be holding.
 */
export interface UploadResult {
  accepted: ScanAccepted
  /** Files this library already indexed at that path: not sent, not committed. */
  alreadyHere: number
  /** Bytes the store already held, so the rows were written without a transfer. */
  bytesSkipped: number
}

/** A file the user picked, with the path it will be known by. */
export interface PickedFile {
  file: File
  /**
   * The browser's own relative path, INCLUDING the dropped folder's own first segment:
   * dropping `brackets/` gives `brackets/steel/LP-1042-03.stl`.
   *
   * Kept rather than stripped, deliberately. Dropping `brackets/` and `plates/` together
   * when each holds a `bracket.stl` collides at `bracket.stl` if the root goes, and
   * `part_source_path_unique_per_library` turns that collision into one part indexed and
   * one silently skipped — the exact failure §2 of the slice design exists to remove.
   *
   * This means a folder dropped here and the same tree scanned from `/ingest` do not
   * produce identical `source_path`s: the scan's root is a server mount whose name is not
   * part of any path, and a drop's root is a folder the user chose and named. They are
   * different actions with different roots, and the one that keeps the user's own naming
   * is the one that loses nothing.
   */
  path: string
}

/** One file's BLAKE3, read incrementally off disk.
 *
 *  `file.stream()` and not `arrayBuffer()`: the largest STL in the owner's corpus is
 *  380 MB, and reading it whole to hash it would put the tab where the api container was
 *  before the download route learned to stream. */
export async function hashFile(file: File): Promise<BlobHash> {
  const hasher = await createBLAKE3()
  hasher.init()
  const reader = file.stream().getReader()
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    hasher.update(value)
  }
  return hasher.digest('hex') as BlobHash
}

/**
 * One file's bytes, in chunks, resuming from wherever the server already is.
 *
 * The offset is never assumed. A `409` carries the length the server actually holds, and
 * that is where the next chunk goes — which covers a retried chunk that in fact landed, a
 * transfer interrupted and started again, and an api that restarted mid-upload, all with
 * the same three lines and no session state anywhere.
 */
async function transfer(
  library: LibraryId,
  blake3: BlobHash,
  file: File,
  onChunk: (bytes: number) => void,
): Promise<void> {
  let offset = 0
  while (offset < file.size) {
    const end = Math.min(offset + CHUNK_BYTES, file.size)
    const { received } = await putChunk(library, blake3, offset, file.slice(offset, end))
    if (received === offset) {
      // The server accepted nothing and did not move: retrying the same offset forever is
      // the one shape this loop must not take.
      throw new Error(`upload of ${file.name} stalled at byte ${offset}`)
    }
    onChunk(Math.max(0, received - offset))
    offset = received
  }
}

/**
 * The whole drop: hash everything, ask what is needed, send only that, commit once.
 *
 * Sequential throughout, per file. Hashing 1,700 files with `Promise.all` opens 1,700
 * read streams at once, which is where a real corpus wedges the tab rather than
 * finishing; and the transfer is sequential because a drop is bounded by disk and
 * bandwidth either way, and a single ordered pass is what makes `bytesSent` mean
 * something.
 */
export async function uploadFiles(
  library: LibraryId,
  picked: PickedFile[],
  onProgress: (progress: UploadProgress) => void,
): Promise<UploadResult> {
  const progress: UploadProgress = {
    phase: 'hashing',
    filesDone: 0,
    filesTotal: picked.length,
    bytesSent: 0,
    bytesToSend: 0,
    alreadyHere: 0,
    bytesSkipped: 0,
  }
  const report = () => onProgress({ ...progress })
  report()

  const hashed: { picked: PickedFile; blake3: BlobHash }[] = []
  for (const one of picked) {
    hashed.push({ picked: one, blake3: await hashFile(one.file) })
    progress.filesDone += 1
    report()
  }

  progress.phase = 'probing'
  progress.filesDone = 0
  report()
  const manifest: UploadFile[] = hashed.map(({ picked, blake3 }) => ({
    path: picked.path,
    blake3,
  }))
  const plan = await probeUpload(library, manifest)

  // The probe answers by path, so the three lists become one lookup rather than three
  // scans of the drop.
  const needBytes = new Set(plan.needBytes)
  const have = new Set(plan.have)
  const toSend = hashed.filter(({ picked }) => needBytes.has(picked.path))
  const toCommit = manifest.filter((file) => !have.has(file.path))

  progress.alreadyHere = plan.have.length
  progress.bytesSkipped = hashed
    .filter(({ picked }) => plan.needRows.includes(picked.path))
    .reduce((total, { picked }) => total + picked.file.size, 0)
  progress.phase = 'transferring'
  progress.filesTotal = toSend.length
  progress.bytesToSend = toSend.reduce((total, { picked }) => total + picked.file.size, 0)
  report()

  for (const { picked, blake3 } of toSend) {
    await transfer(library, blake3, picked.file, (bytes) => {
      progress.bytesSent += bytes
      report()
    })
    progress.filesDone += 1
    report()
  }

  progress.phase = 'committing'
  report()
  return {
    accepted: await commitUpload(library, toCommit),
    alreadyHere: progress.alreadyHere,
    bytesSkipped: progress.bytesSkipped,
  }
}

/**
 * Everything under a dropped folder, with the paths the browser reports.
 *
 * `DataTransferItem.webkitGetAsEntry` is the only way to read a *folder* out of a drop —
 * `DataTransfer.files` holds the folder itself as a zero-byte entry and none of its
 * contents. A dropped file is still a file and is taken as one, so dragging a handful of
 * STLs works alongside dragging the folder that holds them.
 *
 * `readEntries` returns AT MOST 100 entries per call and signals the end with an empty
 * array — it does not return the whole directory. Calling it once reads the first 100
 * files of a folder and silently drops the rest, which on a real parts library is most of
 * it. Hence the loop, which is the single most important line in this function.
 */
export async function filesFromDrop(items: DataTransferItemList): Promise<PickedFile[]> {
  const roots: FileSystemEntry[] = []
  for (const item of Array.from(items)) {
    const entry = item.webkitGetAsEntry()
    if (entry !== null) {
      roots.push(entry)
    }
  }
  const picked: PickedFile[] = []
  for (const root of roots) {
    await walk(root, picked)
  }
  return picked
}

async function walk(entry: FileSystemEntry, into: PickedFile[]): Promise<void> {
  if (entry.isFile) {
    const file = await new Promise<File>((resolve, reject) =>
      (entry as FileSystemFileEntry).file(resolve, reject),
    )
    // `fullPath` is absolute within the drop (`/brackets/steel/x.stl`); `source_path` is
    // relative, and the server refuses a leading slash outright.
    into.push({ file, path: entry.fullPath.replace(/^\/+/, '') })
    return
  }
  if (!entry.isDirectory) {
    return
  }
  const reader = (entry as FileSystemDirectoryEntry).createReader()
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject),
    )
    if (batch.length === 0) {
      return
    }
    for (const child of batch) {
      await walk(child, into)
    }
  }
}

/**
 * Everything from an `<input type="file" webkitdirectory>`, which reports the same paths
 * by a different name. A plain multi-file input has no `webkitRelativePath` at all, so
 * the file's own name is the path — a flat drop of loose files, which is exactly what it
 * is.
 */
export function filesFromInput(files: FileList): PickedFile[] {
  return Array.from(files).map((file) => ({
    file,
    path: file.webkitRelativePath === '' ? file.name : file.webkitRelativePath,
  }))
}
