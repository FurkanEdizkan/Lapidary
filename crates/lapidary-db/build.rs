//! Rebuild this crate whenever `migrations/` changes.
//!
//! `sqlx::migrate!` embeds the migration files at compile time, and it already emits
//! enough tracking for cargo to notice an *edit* to a file it found. It cannot notice a
//! file it never saw: adding `0003_*.sql` changes only the directory's own mtime, so
//! without this cargo considers the crate fresh, the old migration set stays embedded,
//! and `cargo test` runs green against a schema that is not what `migrations/` says. A
//! lying instrument, and the next thing anyone writes here is slice 2's first migration.
//!
//! Verified before adding this file: editing a `.sql` comment did rebuild; creating a new
//! `0003_probe.sql` compiled nothing at all.
//!
//! `rerun-if-changed` on a directory makes cargo watch the directory itself, which is
//! what catches an added or removed file. Naming any path at all also switches cargo off
//! its default of rerunning this script for any change in the package, so the directory
//! must stay listed even though nothing else here reads it.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=build.rs");
}
