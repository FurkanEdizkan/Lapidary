# Lapidary

A fast, dark-themed, local-first gallery and library manager for 3D-printable models
(STL · 3MF · OBJ). React + Vite frontend (`web/`), Fastify + SQLite backend (`server/`),
optional Rust mesh sidecar (`rust-mesh/`) and Redis cache.

## Design Context

This project uses [Impeccable](.claude/skills/impeccable) for frontend design. Before working
on any UI under `web/`, read the design context:

- **[PRODUCT.md](PRODUCT.md)** — strategic: register (`product`), users, purpose, brand
  personality (*Precise · Fast · Trustworthy*), anti-references, design principles, and the
  WCAG 2.1 AA + reduced-motion + colorblind-safe accessibility bar.
- **[DESIGN.md](DESIGN.md)** — visual system: color palette, typography, components, layout.
  Source of truth for tokens is `web/src/theme.ts` and `web/src/global.css`.

Keep the committed dark identity (page `#121214`, cyan accent `#2cb4f5`, Archivo + JetBrains
Mono). Identity-preservation wins; don't reseed the palette.
