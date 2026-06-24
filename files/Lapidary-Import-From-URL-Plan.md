# Lapidary — Import Metadata from a URL (Feature Plan)

> **Status:** post-v1.0 backlog. Pairs with the AI auto-tagging feature as its **manual
> fallback**: when auto-labeling is wrong or thin, the user pastes a source link
> (MyMiniFactory, Thingiverse, Printables, MakerWorld, Cults, Thangs, or any page) and Lapidary
> extracts metadata to fill the gaps.
>
> **Core principle:** this is **single-item, user-initiated enrichment** — never bulk scraping
> to recreate a catalog (which MyMiniFactory and Thingiverse ToS forbid). And it **never
> overwrites silently** — it proposes, the user confirms per field.

---

## 1. Feasibility per source

| Source | Path | Notes |
|---|---|---|
| **MyMiniFactory** | Official API v2 (`/api/v2/object/{id}`), API key or OAuth2 | Richest: name, description, tags, `source_url`, license, creator. **Their guidelines:** link back to the object page, credit the creator's profile, respect the object's license, don't scrape to recreate/compete. |
| **Thingiverse** | Official REST API (`api.thingiverse.com/things/{id}`), app token (read) | name, creator (+ public_url), license, tags, images, description. |
| **Printables / MakerWorld / Cults / Thangs / anything** | **Generic fallback**: Open Graph + schema.org JSON-LD + oEmbed | No per-site API needed. Gets title, author, description, image, often license. Less structured than an API but works everywhere. |

So: **use an official API when the host has one; fall back to generic page-metadata extraction
otherwise.** Confirm current API endpoints/auth against each provider's live docs at
implementation time — they drift.

---

## 2. Architecture — the `MetadataSource` trait

A provider abstraction in a new `crates/lapidary-import` crate (follows the repo's
`modular-services` contract). Providers are tried in priority order; the first whose
`matches()` returns true wins; `GenericSource` is the always-matching catch-all.

```rust
#[async_trait]
pub trait MetadataSource: Send + Sync {
    fn name(&self) -> &'static str;
    /// Does this provider handle the URL? (host + path-shape match)
    fn matches(&self, url: &Url) -> bool;
    /// Fetch and normalize metadata. Must be safe against hostile URLs (see §6).
    async fn fetch(&self, url: &Url) -> Result<ImportedMetadata, ImportError>;
}

pub struct ImportedMetadata {
    pub title:        Option<String>,
    pub creator:      Option<String>,
    pub creator_url:  Option<String>,
    pub description:  Option<String>,   // sanitized: HTML stripped, length-capped
    pub tags:         Vec<String>,
    pub license:      Option<String>,
    pub source_url:   Url,              // canonical object page — stored for attribution
    pub images:       Vec<ImageRef>,    // candidate preview/printed images (url + attribution)
    pub provider:     &'static str,
    pub confidence:   Confidence,       // Api > StructuredData(JSON-LD) > OpenGraph > Oembed
}
```

**Providers:**
- `MyMiniFactorySource` — parse the object id from the URL, call API v2 with a stored key.
- `ThingiverseSource` — parse `thing:{id}`, call `api.thingiverse.com` with an app token.
- `GenericSource` — fetch the page, then extract in this precedence:
  1. **schema.org JSON-LD** (`CreativeWork` / `3DModel`: `name`, `author`, `license`, `image`)
  2. **Open Graph** (`og:title`, `og:description`, `og:image`, article author)
  3. **oEmbed** (if the host advertises an endpoint)

The registry resolves a URL → the best provider, calls `fetch`, and returns one normalized
`ImportedMetadata` regardless of source, so the UI is identical everywhere.

---

## 3. Field mapping → Lapidary model

Requires two schema additions to `models` (these were already flagged as quick wins):
`source_url TEXT` and `license TEXT` (optionally `creator_url TEXT`). Migration
`00NN_model_source_license`.

| Imported | Model field | Merge rule |
|---|---|---|
| title | name | propose; user opts in (don't clobber a name they set) |
| creator / creator_url | creator (+ creator_url) | propose |
| description | notes | sanitized, propose |
| tags | tags | **merge** (union), never replace |
| license | license | propose; surface restrictive licenses (e.g. non-remixable) prominently |
| source_url | source_url | always store on apply (attribution requirement) |
| images | printed-results / backdrop candidates | opt-in download (§7, license-gated) |

---

## 4. Human-in-the-loop UX (the crux)

On the Add/Edit model flow and the model page: **"Import from link."**

```
Paste URL  → [Fetch]
   → provider resolved (e.g. "MyMiniFactory · API")
   → PREVIEW panel: each field shows  CURRENT → PROPOSED  with a checkbox
        ☑ Name      Gothic Trench Barricade  →  Gothic Trench Barricade (Wastes)
        ☑ Creator   (empty)                  →  TrenchWorks  ↗
        ☑ Tags      terrain, wargaming       →  + trench-crusade, + gothic   (merge)
        ☐ Notes     (keep mine)              →  "A ruined barricade…"
        ☑ License   (empty)                  →  CC-BY-NC  ⚠ non-commercial
        ☑ Source    (empty)                  →  myminifactory.com/object/…  ↗
   → [Apply selected]   (only ticked fields are written)
```

Rules: never overwrite a non-empty field without the user ticking it; tags **merge**; show a
license warning badge when the imported license is restrictive; the source link + creator are
always retained on apply (attribution).

---

## 5. Where it runs

- The fetch + preview is a **synchronous** `lapidary-server` endpoint (fast; one request).
- Optional **image download** is an async `lapidary-worker` job (so a slow CDN doesn't block
  the UI), writing into the existing images/ tier with attribution recorded.

---

## 6. Compliance & guardrails (non-negotiable)

- **SSRF protection** — the server fetches an arbitrary user-supplied URL, so: allow only
  `http`/`https`; resolve the host and **reject private/loopback/link-local/metadata ranges**
  (127/8, 10/8, 172.16/12, 192.168/16, 169.254/16, `::1`, cloud metadata IPs); cap redirects;
  enforce timeouts and a max response size; re-check the IP after each redirect. `security-auditor`
  owns this.
- **Attribution & license** — always store and display the `source_url` and creator; record and
  surface the object license; honor restrictive flags (non-remixable / non-commercial).
- **Not a scraper** — single, user-initiated lookups only, rate-limited and cached. Do **not**
  crawl, bulk-import, or mirror a provider's catalog. State this in code comments and docs so a
  future contributor doesn't "optimize" it into a crawler and breach ToS.
- **Generic fallback hygiene** — honor `robots.txt`, send a descriptive User-Agent identifying
  Lapidary, strip/sanitize HTML from descriptions, validate parsed data before proposing.
- **API keys** (MMF, Thingiverse) stored **encrypted** in the `secrets` table (reuse the
  encryption from the send-to-printer plan); never logged, never in model JSON.

---

## 7. Optional image import (MI-D)

If the user opts in, download the provider's preview/printed image(s) into the images/ tier
to power the detail-page backdrop and printed-results strip — **but** record attribution and
respect the license (don't import images under a license that forbids reuse; warn and skip).
Off by default.

---

## 8. Phasing

- **MI-A — universal first.** `lapidary-import` crate, `MetadataSource` trait, `GenericSource`
  (JSON-LD + OG + oEmbed), the preview/confirm UX, `source_url`/`license` schema, and the SSRF
  guard. Works on **every** site immediately, no API keys.
- **MI-B — MyMiniFactory provider** (your example): API v2 + stored key → richest data.
- **MI-C — Thingiverse provider**: app-token read access.
- **MI-D — optional image import** with attribution + license gating.

---

## 9. Acceptance criteria

- Pasting a MyMiniFactory / Thingiverse / Printables URL yields a normalized preview; the
  provider used is shown (API vs page-metadata).
- The preview shows current→proposed per field; applying writes **only** ticked fields; tags
  merge; existing values aren't clobbered unticked.
- `source_url` + license + creator are stored and displayed; restrictive licenses are flagged.
- SSRF: a URL pointing at `localhost`/a private IP/a cloud metadata endpoint is refused; redirect
  to a private IP is refused.
- API keys are encrypted at rest and absent from all logs/responses.
- All gates green (`cargo fmt/clippy/test`).

---

## 10. Agents

`codebase-analyst` (confirm current MMF/Thingiverse API shapes), `implementer` (crate +
providers + UX wiring), `test-engineer` (provider tests against recorded fixtures, SSRF test
matrix, never hit live providers in CI), `security-auditor` (SSRF, secret handling, ToS/license
compliance), `frontend-ux-reviewer` (the preview/confirm panel), `backend-reviewer`,
`code-quality-reviewer`.
