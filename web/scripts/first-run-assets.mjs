// Fetches the first-run scene's models: the L0 rung of three example parts, from a running stack.
//
//   node web/scripts/first-run-assets.mjs [http://127.0.0.1:8080]
//
// The rungs are what the worker writes for `example/parts` on a fresh install (meshopt-compressed
// glTF, `glb.rs`), committed under `web/public/first-run/` so an empty library can show real parts
// without a library to read them from. Run it again when `glb.rs` changes what a rung holds.
import { mkdirSync, writeFileSync } from 'node:fs'

const api = process.argv[2] ?? 'http://127.0.0.1:8080'
const MODELS = ['spur-gear', 'flange', 'vee-block']
const out = new URL('../public/first-run/', import.meta.url)
mkdirSync(out, { recursive: true })

const libraries = await (await fetch(`${api}/api/libraries`)).json()
const parts = []
for (const library of libraries) {
  const page = await (await fetch(`${api}/api/libraries/${library.id}/parts?limit=250`)).json()
  parts.push(...page.parts)
}
for (const model of MODELS) {
  const part = parts.find((candidate) => candidate.name.startsWith(model) && candidate.tessellationL0 !== null)
  if (part === undefined) throw new Error(`no example part named ${model}… with a rung; is example/parts seeded on ${api}?`)
  const response = await fetch(`${api}/api/blob/${part.tessellationL0}`)
  if (!response.ok) throw new Error(`the rung of ${part.name} answered ${response.status}`)
  const bytes = new Uint8Array(await response.arrayBuffer())
  writeFileSync(new URL(`${model}.glb`, out), bytes)
  console.log(`${model}.glb  ${bytes.length} bytes  from ${part.name}`)
}
