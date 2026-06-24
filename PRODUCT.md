# Product

## Register

product

## Users

3D-printing makers and hobbyists managing a personal — sometimes shared — library of
printable models (STL · 3MF · OBJ) on their own hardware. They use Lapidary at a desk,
often side-by-side with a slicer, while deciding what to print next, organizing a growing
collection, or checking whether a model fits their printer and dialed-in settings.

The job to be done: **turn a pile of mesh files into a collection you can actually navigate** —
find the right model in seconds, inspect it in an interactive 3D viewer, and keep everything
tagged, pinned, grouped, and searchable with metadata trustworthy enough to act on.

## Product Purpose

Lapidary is a fast, local-first gallery and library manager for 3D-printable models. It runs
on your own machine (npm, single web server, or Docker/Podman) and presents an **image-first
grid** (Grid / Cards / List) backed by an interactive Three.js viewer and rich per-model
metadata: size, creator, dates, type, compatible printers, suggested print settings (importable
from PrusaSlicer / Orca / Cura profiles), and photos of the printed/painted result.

A three-tier storage model (compressed original → decimated LOD → thumbnail) keeps the gallery
instant; the full mesh is read only on demand. Optional Redis and an optional Rust mesh sidecar
accelerate the experience but always degrade gracefully.

**Success looks like:** the library feels instantaneous, any model is one search-and-click away,
and the metadata is accurate enough that the user trusts it when sending a model to the printer.

## Brand Personality

**Precise · Fast · Trustworthy.** Instrument-like and confident — dense but always legible,
the way a well-made tool feels in the hand. The voice is terse and accurate with no marketing
fluff; labels read like a professional instrument, not a consumer app. The emotional goal is
quiet confidence: the tool disappears into the task and lets the models be the focus.

## Anti-references

What Lapidary must **not** look like:

- **Consumer marketplace clutter** — Thingiverse / Printables-style ad-heavy, infinite-scroll
  grids with promo cruft and social noise. Keep it clean and personal, not a storefront.
- **Heavy enterprise CAD/PLM** — SolidWorks / PLM dense gray toolbars, nested panels, and
  heavyweight coldness. Lapidary is precise without being industrial-bleak.
- **Generic SaaS dashboard** — purple gradients, identical card grids, the hero-metric template,
  and tracked-uppercase eyebrows above every section. The AI-slop look is disqualifying.
- **Playful / childish maker vibes** — cartoonish rounded shapes, emoji-heavy chrome, toy-like
  styling that undermines the pro-tool credibility.

## Design Principles

1. **The tool disappears into the task.** Earned familiarity over novelty — standard affordances
   for standard jobs (search, modal, table, viewer). Surprise is spent on moments, never on chrome.
2. **Image-first and instant.** The model is the hero. The library must feel immediate; serve the
   thumbnail and LOD first, full mesh only on intent. Perceived speed is a design requirement.
3. **Trust the metadata.** Every spec shown is accurate and legible enough to act on. The interface
   never overstates what's known or available — if a capability is off (Redis, sidecar), say so honestly.
4. **Density with calm.** Show a lot without shouting. Rhythm, restraint, and a tight type scale
   carry information density; clutter is a failure, not a feature.
5. **Degrade gracefully, visibly.** Optional accelerators are optional. The UI stays fully functional
   on the fallback path and tells the truth about which path it's on.

## Accessibility & Inclusion

Target **WCAG 2.1 AA**.

- **Contrast:** body text ≥ 4.5:1 and large / bold UI text ≥ 3:1 against the dark surfaces;
  placeholder and muted text held to the same body threshold (no decorative low-contrast gray).
- **Color independence (colorblind-safe):** never rely on the cyan accent alone to convey state.
  Selection, active, error, and success must carry a second cue — shape, icon, label, weight, or border.
- **Reduced motion:** every transition and reveal has a `prefers-reduced-motion: reduce` alternative
  (crossfade or instant). No motion is required to understand or operate the interface.
- **Keyboard & focus:** all interactive surfaces are keyboard-reachable with a visible focus ring;
  the search, viewer, and modals trap and restore focus correctly.
