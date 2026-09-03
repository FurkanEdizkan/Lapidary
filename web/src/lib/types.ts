/**
 * Re-exports of the ts-rs output in ../bindings. Import domain types from here, never
 * from ../bindings directly — this is the one file that fails to compile when a Rust
 * type is renamed or removed.
 *
 * Phase 1 is the first real consumer: the grid renders PartsPage/PartCard straight off
 * the wire. Nothing may hand-write a matching interface instead — a duplicate compiles
 * happily after the Rust type changes underneath it, which is exactly what the bindings
 * staleness gate exists to prevent.
 */
export type { Approximate } from '../bindings/Approximate'
export type { BlobHash } from '../bindings/BlobHash'
export type { LibraryId } from '../bindings/LibraryId'
export type { LibraryMode } from '../bindings/LibraryMode'
export type { PartCard } from '../bindings/PartCard'
export type { PartId } from '../bindings/PartId'
export type { PartsPage } from '../bindings/PartsPage'
export type { PartSummary } from '../bindings/PartSummary'
export type { RevisionId } from '../bindings/RevisionId'
