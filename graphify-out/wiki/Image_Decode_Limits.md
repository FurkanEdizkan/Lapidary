# Image Decode Limits

> 27 nodes

## Key Concepts

- **upload.rs** (21 connections) — `crates/lapidary-api/src/upload.rs`
- **chunk()** (18 connections) — `crates/lapidary-api/src/upload.rs`
- **commit()** (14 connections) — `crates/lapidary-api/src/upload.rs`
- **store_staged()** (14 connections) — `crates/lapidary-api/src/upload.rs`
- **probe()** (11 connections) — `crates/lapidary-api/src/upload.rs`
- **.into_response()** (8 connections) — `crates/lapidary-api/src/upload.rs`
- **library_exists()** (8 connections) — `crates/lapidary-api/src/upload.rs`
- **bad_request()** (8 connections) — `crates/lapidary-api/src/upload.rs`
- **part.rs** (8 connections) — `crates/lapidary-core/src/part.rs`
- **staged_path()** (7 connections) — `crates/lapidary-api/src/upload.rs`
- **refuse_chunk()** (6 connections) — `crates/lapidary-api/src/upload.rs`
- **UploadFile** (5 connections) — `crates/lapidary-api/src/upload.rs`
- **UploadManifest** (5 connections) — `crates/lapidary-api/src/upload.rs`
- **append_chunk()** (5 connections) — `crates/lapidary-api/src/upload.rs`
- **ChunkRefusal** (4 connections) — `crates/lapidary-api/src/upload.rs`
- **source_format()** (4 connections) — `crates/lapidary-core/src/part.rs`
- **Bytes** (3 connections)
- **IntoResponse** (3 connections)
- **ChunkQuery** (2 connections) — `crates/lapidary-api/src/upload.rs`
- **BytesRejection** (2 connections)
- **LibraryMode** (2 connections) — `crates/lapidary-core/src/part.rs`
- **path_escapes()** (2 connections) — `crates/lapidary-core/src/part.rs`
- **ChunkAccepted** (1 connections) — `crates/lapidary-api/src/upload.rs`
- **.as_str()** (1 connections) — `crates/lapidary-core/src/part.rs`
- **an_ordinary_nested_path_does_not_escape()** (1 connections) — `crates/lapidary-core/src/part.rs`
- *... and 2 more nodes in this community*

## Relationships

- [[Scan Enqueue]] (15 shared connections)
- [[part table]] (9 shared connections)
- [[Library Settings and Thumbnails]] (9 shared connections)
- [[Storage Paths and IO]] (7 shared connections)
- [[Folder and Part Identifiers]] (6 shared connections)
- [[Soft Delete Subtree]] (6 shared connections)
- [[Folder Tree Drag Tests]] (5 shared connections)
- [[Prototype Image Slot]] (2 shared connections)
- [[Directory Scan Cells]] (1 shared connections)
- [[SSRF Fetch Guard]] (1 shared connections)
- [[Folder Routes]] (1 shared connections)
- [[ModelManifest]] (1 shared connections)

## Source Files

- `crates/lapidary-api/src/upload.rs`
- `crates/lapidary-core/src/part.rs`

## Audit Trail

- EXTRACTED: 158 (96%)
- INFERRED: 7 (4%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*