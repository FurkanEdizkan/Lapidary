# Lapidary — Send-to-Printer Feature Plan

> **Status:** post-v1.0 backlog. **Do not** start this during Phases 1–6 of the Rust migration;
> it is net-new scope and would compromise the parity goal. Slot it in after cutover.
>
> **What this delivers:** a "Send to printer" action on a model that pushes a printable file to
> OctoPrint, Klipper/Moonraker, PrusaLink, or a Bambu Lab printer — from inside Lapidary.

---

## 1. The two realities this design is built around

1. **Printers consume G-code, not STL.** Lapidary stores source meshes (STL/3MF/OBJ), so every
   send implies either a *pre-sliced* file already in the library or an on-the-fly **slice
   step**. The slice step is a separate concern from the transport (see §4 and §5).
2. **Bambu locked down third-party print-start in Jan 2025.** Starting a print from non-Bambu
   software requires **Bambu Connect** middleware, *or* the printer's optional **Developer
   Mode** (LAN-only, opens MQTT/FTP, user-enabled, unofficial). The open ecosystems
   (OctoPrint/Moonraker/PrusaLink) have no such restriction.

These split the work into a **slicing** layer and a **transport** layer that compose:
`Slicer.slice(mesh, profile, target) → artifact` then `PrintTarget.send(artifact)`.

---

## 2. Transport layer — the `PrintTarget` trait

One driver per ecosystem behind a single trait (follows the repo's `modular-services`
contract: typed in → typed out, one entrypoint per unit). Lives in a new
`crates/lapidary-print` crate, depended on by `lapidary-server` (for actions) and
`lapidary-worker` (for status polling).

```rust
#[async_trait]
pub trait PrintTarget: Send + Sync {
    fn kind(&self) -> TargetKind;
    /// What this target can actually do — UI greys out unsupported actions.
    fn capabilities(&self) -> Capabilities;      // can_status, can_pause, can_stop, fire_and_forget

    /// Connectivity + machine state. Used to validate a target before sending.
    async fn probe(&self) -> Result<PrinterStatus, PrintError>;

    /// Upload a printable artifact to the target. Returns a remote handle.
    async fn upload(&self, art: &SliceArtifact) -> Result<RemoteFile, PrintError>;

    /// Begin printing an uploaded file. Requires explicit user confirmation upstream.
    async fn start(&self, remote: &RemoteFile, opts: &PrintOptions) -> Result<JobHandle, PrintError>;

    /// Progress for targets that support it (capability-gated).
    async fn status(&self, job: &JobHandle) -> Result<PrintProgress, PrintError>;
}

pub enum TargetKind { OctoPrint, Moonraker, PrusaLink, BambuConnect, BambuLan }
```

A `send()` convenience = `upload` + `start`. **Never auto-start**: `start` is only called after
an explicit user confirm in the UI — a physical machine heats up and moves.

### Drivers

| Driver | Transport | Auth | Status? | Notes |
|---|---|---|---|---|
| `OctoPrintTarget` | REST over HTTP: upload file, then select+print | `X-Api-Key` header | yes | simplest; confirm exact endpoints against current OctoPrint API docs |
| `MoonrakerTarget` | HTTP API: upload G-code, start print | API key / trusted host | yes | Klipper via Moonraker |
| `PrusaLinkTarget` | PrusaLink local REST: upload then print | API key / digest | yes | confirm v1 endpoints against current PrusaLink docs |
| `BambuConnectTarget` | hand-off: launch Bambu Connect via its URL scheme with a sliced 3MF | handled by Connect | no (fire-and-forget) | **sanctioned** path; survives firmware updates; requires Connect installed |
| `BambuLanTarget` | direct LAN: FTPS upload (990) + MQTT/TLS control (8883) with access code; X.509 signing | LAN access code + cert | yes | **advanced, opt-in, unofficial**; needs Developer Mode on; fragile to firmware updates; ToS gray area — gate behind a clear warning |

> For Bambu, the driver picks `BambuConnect` vs `BambuLan` from the target's stored config.
> Default to `BambuConnect`. There is also a community Bambu MCP server doing the LAN path; if
> you ever expose this via your Claude/MCP tooling, treat it as the same advanced/opt-in tier.

---

## 3. Printers registry — schema

A printer **type** (e.g. "Bambu X1C") already exists in `printer_types` /
`model_printer_types`. A **target** is a *connection* — new and distinct. New migration
(e.g. `0003_print_targets`, sequenced after the migration phases):

```sql
CREATE TABLE print_targets (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,                 -- "Garage X1C"
  kind          TEXT NOT NULL,                 -- octoprint|moonraker|prusalink|bambu_connect|bambu_lan
  host          TEXT,                          -- ip/hostname (null for bambu_connect handoff)
  port          INTEGER,
  serial        TEXT,                          -- bambu
  bambu_mode    TEXT,                          -- connect|developer (null for non-bambu)
  secret_ref    TEXT,                          -- pointer into `secrets`, NEVER the secret itself
  default_profile_id INTEGER REFERENCES printer_settings(id), -- slice profile to use
  printer_type_id    INTEGER REFERENCES printer_types(id),    -- optional link to the "type"
  enabled       INTEGER NOT NULL DEFAULT 1,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TABLE secrets (                          -- encrypted at rest; see §6
  ref           TEXT PRIMARY KEY,
  ciphertext    BLOB NOT NULL,                  -- AES-GCM
  nonce         BLOB NOT NULL,
  created_at    TEXT NOT NULL
);

CREATE TABLE print_jobs (                       -- the send/print log (feeds print-history)
  id            TEXT PRIMARY KEY,
  model_id      TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
  target_id     TEXT NOT NULL REFERENCES print_targets(id) ON DELETE CASCADE,
  artifact_hash TEXT,                           -- which slice was sent (content-addressed)
  status        TEXT NOT NULL,                  -- queued|slicing|uploading|printing|done|failed
  remote_job    TEXT,                           -- target's job id where available
  started_at    TEXT,
  finished_at   TEXT,
  outcome       TEXT,                           -- success|failed|cancelled + note
  error         TEXT
);
```

---

## 4. Slicing layer — the `Slicer` service

```rust
#[async_trait]
pub trait Slicer: Send + Sync {
    async fn slice(&self, mesh: &Path, profile: &SlicerProfile, target: TargetKind)
        -> Result<SliceArtifact, SliceError>;   // → G-code (FDM hosts) or sliced 3MF (Bambu)
}
```

- **Driver:** `OrcaSlicerCli` (or PrusaSlicer) spawned **in the worker container** — which
  already carries Blender, so adding a slicer CLI is architecturally consistent. It consumes
  the model's **imported print profile** (you already parse PrusaSlicer/Orca `.ini` / Cura
  `.json`).
- **Output format by target:** G-code for OctoPrint/Moonraker/PrusaLink; a **sliced 3MF** for
  Bambu (plate G-code embedded).
- **Cache the artifact** keyed by `sha256(mesh) + sha256(profile) + target` — reuses your
  content-addressing + tiered-cache pattern, so re-sends are instant and don't re-slice.
- **Pre-sliced shortcut:** if the library already holds a G-code/3MF for `(model, profile,
  target)`, skip slicing entirely and send it.
- **Bambu filament/AMS mapping is the hard part** — the profile must carry AMS slot/filament
  mapping or the print will mis-map colors. Surface this in the UI before send; don't guess.

---

## 5. End-to-end flow

```
[Model page] → "Send to printer"
   → pick a target from the registry (probe() validates it's reachable)
   → resolve artifact:
        if pre-sliced (model,profile,target) exists → use it
        else enqueue a SLICE job on the worker (status: slicing)
   → upload() to the target            (status: uploading)
   → CONFIRM dialog (machine will heat + move)
   → start()  OR  Bambu Connect URL hand-off   (status: printing)
   → status() polling where capable → progress on the model page
   → log to print_jobs (feeds the print-history/"everything I printed" feature)
```

For `BambuConnect` (fire-and-forget) the flow stops at the hand-off; mark the job `printing`
with no further polling, and let the user confirm completion manually.

---

## 6. Secret handling (non-negotiable)

- Access codes / API keys live **only** in the `secrets` table, **encrypted** with AES-GCM
  using a key derived from an env var (`LAPIDARY_SECRET_KEY`); `print_targets.secret_ref`
  points to the row. They are **never** in any model JSON, never in a plaintext column.
- **Redact in logs and errors** — no secret ever reaches `tracing` output or an API response.
- **Path-traversal guard** on every upload path (no `..`, no absolute paths) — the same guard
  the community Bambu tooling applies to FTPS uploads.
- For `BambuLan`, the X.509 cert material and access code are secrets too; the Developer-Mode
  warning must state the user owns their LAN security.

---

## 7. Phasing (each a separable PR)

- **SP-A — foundation + open hosts (ship first).** `lapidary-print` crate, `PrintTarget` trait,
  `print_targets` + `secrets` + `print_jobs` schema, secret encryption, and the
  **OctoPrint** + **Moonraker** drivers operating on **pre-sliced G-code** already in the
  library. Real value, lowest risk, no slicer dependency.
- **SP-B — slice-then-send.** `Slicer` trait + `OrcaSlicerCli` driver in the worker; artifact
  caching; STL→print works end-to-end for the open hosts. Add **PrusaLink** driver.
- **SP-C — Bambu (sanctioned).** `BambuConnectTarget` URL-scheme hand-off with a sliced 3MF.
- **SP-D — Bambu (advanced, opt-in).** `BambuLanTarget` direct MQTT+FTPS under Developer Mode,
  behind an explicit risk warning. Optional MCP exposure for Claude tooling at the same tier.

---

## 8. Acceptance criteria

- **SP-A:** register an OctoPrint + a Moonraker target (secret stored encrypted, never logged);
  `probe()` reports reachable/state; sending a pre-sliced G-code uploads and starts a print
  after explicit confirm; `print_jobs` logs it; status polls where supported.
- **SP-B:** sending an STL with a stored profile slices once, caches the artifact, and a re-send
  reuses the cache (no re-slice); G-code/3MF format chosen correctly per target.
- **SP-C:** Bambu Connect launches via URL scheme with a valid sliced 3MF; job logged as
  fire-and-forget.
- **SP-D:** with Developer Mode on, FTPS upload + MQTT start works with the LAN access code;
  disabled by default behind a warning; secrets encrypted; path-traversal blocked.
- All gates: `cargo fmt/clippy/test` green; no secret in any log or response; no print starts
  without explicit user confirmation.

---

## 9. Risks & guardrails

- **Bambu fragility / ToS.** The LAN/cert-bypass path can break on firmware updates and is a
  gray area — keep it opt-in, default to Connect hand-off, never the default.
- **Slicer profile correctness.** Wrong profile = wasted filament or a crash; AMS/filament
  mapping for Bambu especially. Show the resolved profile + mapping before send.
- **Safety.** Always require explicit confirmation before `start()`. Never auto-print on scan
  or on cache warm.
- **Endpoint drift.** OctoPrint/Moonraker/PrusaLink REST details change; the implementer should
  confirm exact endpoints/auth against each project's *current* API docs rather than hardcoding
  from memory.

---

## 10. Agents

`codebase-analyst` (confirm current OctoPrint/Moonraker/PrusaLink/Bambu APIs), `implementer`
(crate + drivers + slicer), `test-engineer` (mock-server tests per driver; never hit a real
printer in CI), `security-auditor` (secret encryption, path traversal, redaction, the Bambu
opt-in warning), `infra-deployer` (slicer CLI into the worker image), `backend-reviewer`,
`code-quality-reviewer`.
