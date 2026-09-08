---
target: the parts grid
total_score: 22
max_score: 40
na_heuristics: 
p0_count: 1
p1_count: 3
target_identity: "file:/mnt/Storage/All/Develop/Lapidary/web/src/routes/index.tsx"
target_fingerprint: "sha256:39bfde251c3fe583e248bde341d49abb7c68ef96596bc8c02adf185d7dedbb80"
target_path: /mnt/Storage/All/Develop/Lapidary/web/src/routes/index.tsx
timestamp: 2026-09-08T06-54-35Z
slug: web-src-routes-index-tsx
---
Method: dual-agent (A: design review, re-run after a session restart · B: detector + browser evidence). Isolated; A never saw B.

## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 2 | Grid never states library size; partCount 156 already cached |
| 2 | Match System / Real World | 3 | Copy outstanding; ≈, "Watertight: Closed", kernel version leak to a hobbyist |
| 3 | User Control and Freedom | 2 | Quick-look has no close button, no scrim dismissal; Escape only |
| 4 | Consistency and Standards | 2 | Three button treatments; whole-card click has cursor:auto |
| 5 | Error Prevention | 3 | movable guard and tri-state checkbox strong; drag-to-move has no undo |
| 6 | Recognition Rather Than Recall | 2 | Everything meaningful behind hover, 77px of it clipped |
| 7 | Flexibility and Efficiency | 2 | 5 tab stops per card; part #50's name is 262 Tabs away |
| 8 | Aesthetic and Minimalist Design | 2 | 387px chrome before first render; only saturated pixel is an admin checkbox |
| 9 | Error Recovery | 3 | Best category in the app; zero-results offers no recovery |
| 10 | Help and Documentation | 1 | Help is title-attribute-only: invisible to touch and keyboard |
| **Total** | | **22/40** | **Acceptable, bottom of band** |

No heuristic scored n/a; all ten apply to an Operate surface.

## Design Specificity Verdict

The brand is in the strings file, not on the screen. Genuinely Lapidary: deleteBody composing from real counts and stating what does not happen; "Show storage path" refusing an idiom a browser cannot honour; Figure unable to render a value without provenance. But strip the copy and it is a generic admin CRUD page. The card's reveal panel covers the picture you were deciding from — a visual index whose interaction model is "hide the visual".

Agreement between assessments: the 10.4px APPROXIMATE badge (B: 50 findings, one source line; A: P0 consequence) and accent-at-rest (B: exactly 1 element paints #6ea8fe, an admin checkbox; A: violates DESIGN.md's Marking Dye Rule).

Split earned its keep: B measured text contrast — zero failures, min 7.52:1, clears AAA — and noted verdict-free that the badge border is 1.22:1. A identified that as SC 1.4.11 non-text contrast, 3:1 required, 1.31:1 delivered, on every input, select, drop target and card boundary.

B was blind to the clipping: it asserted rest state and measured at rest; the clipping exists only on hover.

False positives correctly discarded by B: two broken-image hits in no-bare-strings.test.ts (assertion strings, not markup); one dark-glow that was the detector flagging its own overlay.

## Priority Issues

[P0] Reveal panel clips 77px, taking the mandated Approximate label and the Download link.
Content 255.9px in a 179px well. Measurement row 100% clipped at every desktop width. Download link 96% clipped and is the first Tab stop on every card. Storage path open reaches 430px, losing the path's first 35px. CLAUDE.md requires the approximate label always; on the grid it is invisible. Root cause of the missed verification: auto-fill with minmax(11rem,1fr) packs MORE columns as the viewport widens, so tiles shrink on bigger screens — 218px wells at 1194px, 186px at 1904px. The fix was confirmed at 1194px, the roomiest case. Suggested command: /impeccable adapt

[P1] Quick-look cannot be closed with a mouse. No close control; Dialog's scrim has no onClick. Escape works, signposted nowhere. Unusable on touch. Suggested command: /impeccable harden

[P1] Zero-results shows the wrong heading and no exit. index.tsx:1251 renders categoryTitle "Nothing filed here yet" unconditionally, even with All models selected. Recovery button gated on !filtered, so unfiltered there is no control. Suggested command: /impeccable clarify

[P1] WCAG 2.2 AA fails on non-text contrast and target size. Borders 1.31:1 vs 3:1 required (SC 1.4.11); "New category" 22px and "Removed parts" 20px vs 24px (SC 2.5.8). DESIGN.md's hairline rule and PRODUCT.md's AA commitment conflict; accessibility loses. Suggested command: /impeccable audit

[P2] Primary interaction has no affordance and right-click is taken. cursor:auto on a clickable draggable card; onContextMenu preventDefault kills open-in-new-tab. Suggested command: /impeccable polish

## Minor Observations

- --duration-fast is dead application-wide: every element carrying it computes to 0.18s. The unlayered * rule in styles.css beats the layered utility. Fix: wrap the * rule in @layer base.
- document.title is "Lapidary"; three tabs are indistinguishable.
- Quick-look renders the part name twice as sibling h2s.
- Dialog focuses its container, drawing the accent ring around the whole 448px box.
- The grid never says 156 though the count is cached.

## Persona Red Flags

Alex (power user): 264 focusables; part name is the last of 5 tab stops per card. No shortcuts, no /-to-search, no bulk select. Right-click hijack breaks opening parts into tabs.

Sam (accessibility-dependent): first Tab stop on every card is a 96%-clipped Download link. The article has onClick with no role, tabIndex or key handler — quick-look is mouse-only. ul.list-none without role="list" drops list semantics in VoiceOver.

The Maker with 1,700 STLs (PRODUCT.md): 387px of setup controls before the first picture. Hovering a part hides the part. Search failure is a dead end with no count, scope or clear control.

## Questions to Consider

- What if the card had no hover panel at all — render, name, Approximate badge, and every action in the quick-look you already built?
- Why is the quick-look a 448px modal instead of the grid's second pane, the shape every tool this audience uses already has?
- What is the operator chrome doing on the hobbyist's screen forty times a day?
- Is ≈ a label, or a symbol you are hoping people learn? CLAUDE.md says "labelled approximate".
