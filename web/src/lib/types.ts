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
export type { BatchId } from '../bindings/BatchId'
export type { BatchStatus } from '../bindings/BatchStatus'
export type { BlobHash } from '../bindings/BlobHash'
export type { FolderId } from '../bindings/FolderId'
export type { FolderNode } from '../bindings/FolderNode'
export type { FolderPatch } from '../bindings/FolderPatch'
export type { InstanceStorageView } from '../bindings/InstanceStorageView'
export type { PartImage } from '../bindings/PartImage'
export type { StoredImage } from '../bindings/StoredImage'
export type { NewFolder } from '../bindings/NewFolder'
export type { LibraryId } from '../bindings/LibraryId'
export type { LibraryMode } from '../bindings/LibraryMode'
export type { LibrarySettings } from '../bindings/LibrarySettings'
export type { LibraryStorage } from '../bindings/LibraryStorage'
export type { MovePart } from '../bindings/MovePart'
export type { PartCard } from '../bindings/PartCard'
export type { PartDetail } from '../bindings/PartDetail'
export type { PartId } from '../bindings/PartId'
export type { PartsPage } from '../bindings/PartsPage'
export type { PurgeResult } from '../bindings/PurgeResult'
export type { PartSummary } from '../bindings/PartSummary'
export type { RevisionId } from '../bindings/RevisionId'
export type { ScanAccepted } from '../bindings/ScanAccepted'
export type { ChunkAccepted } from '../bindings/ChunkAccepted'
export type { UploadFile } from '../bindings/UploadFile'
export type { UploadManifest } from '../bindings/UploadManifest'
export type { UploadPlan } from '../bindings/UploadPlan'
