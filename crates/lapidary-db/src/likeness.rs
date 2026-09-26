//! Which parts of a library look alike, and what somebody decided about a pair (Phase 6; design in
//! `docs/goals/phase-6.md` §§ Three kinds of alike; Links, and folding).
//!
//! The SQL here only fetches candidates: a library's current shape profiles, its live parts and the
//! bytes each came from, and the `part_link` rows. Every judgement — what is close, what is a
//! near-duplicate, which parts belong in one cluster — is a pure function below, unit-tested without a
//! database. That split is the goal's rule, and it is also what makes the ranking cheap to change: the
//! thresholds live in `lapidary_core::shape` and nothing in this file knows a number.
//!
//! **"Current" profile means [`SHAPE_VERSION`] and the part's newest revision.** `part_shape` also
//! records the L0 tessellation's hash, and `shapes.rs` calls a row stale when that hash is not the
//! current L0's; this module does not check that last part, because it needs the L0 derivative of the
//! current revision — a second lateral a row on the path with a 50 ms budget — and catches one case
//! only: an L0 rebuilt for the same revision without a [`SHAPE_VERSION`] bump. G2's profile job is
//! what notices that and writes the row again. One definition, used for every read here, so
//! `profiled`, `unprofiled` and both routes agree about what has been compared.
//!
//! ponytail: every read here is an exact scan over `real[]`. The owner's Phase 6 decision says that
//! beats an approximate index up to about 100k parts a library; past that the upgrade is
//! `ALTER TABLE part_shape ALTER descriptor TYPE vector(35) USING descriptor::vector` and an HNSW
//! index, plus moving the test databases to `deploy/db`'s image.

use crate::DbError;
use crate::repo::detail_stamp;
use jiff::Timestamp;
use lapidary_core::shape::{distance, is_near_duplicate, size_band};
use lapidary_core::{
    BlobHash, DESCRIPTOR_LEN, LibraryId, PartId, PartLinkKind, SHAPE_VERSION, ShapeProfile,
};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// One live part of a library, whether or not it has been profiled: what it is, when it arrived, and
/// the bytes its current revision came from.
///
/// `source` is `None` for a revision with no source file row — a state the grid already tolerates —
/// and such a part is never identical to anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePart {
    pub part: PartId,
    pub created_at: Timestamp,
    pub source: Option<BlobHash>,
}

/// One part's current shape profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Shaped {
    pub part: PartId,
    pub profile: ShapeProfile,
}

/// One `part_link` row, as either side reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkRow {
    pub part: PartId,
    pub other: PartId,
    pub kind: PartLinkKind,
}

impl LinkRow {
    /// The other part, given the one being looked at; `None` when this row is about neither.
    pub fn facing(&self, part: PartId) -> Option<PartId> {
        if self.part == part {
            Some(self.other)
        } else if self.other == part {
            Some(self.part)
        } else {
            None
        }
    }
}

/// Why a fold did not happen. Each maps to one status in `lapidary-api`'s handler, and the reasons a
/// caller must not be able to tell apart share a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folded {
    Done,
    /// Either part is gone, removed already, or in another library. One variant because all three are
    /// a `404` that says the same thing: nothing here changed.
    NoSuchPair,
    /// A part cannot be folded into itself.
    IntoItself,
}

pub struct PgLikeness(pub PgPool);

impl PgLikeness {
    /// Every live part of this library that has a current profile, smallest `size_mm` first.
    ///
    /// Read straight off `part_shape_size`, with one lookup on `revision_part_id` a row to check that
    /// no revision newer than the profiled one exists. **Newer is `id >`**, because revision ids are
    /// UUID v7 and sort by time — the same invariant [`crate::PgShapes::record`]'s own
    /// `revision_id <= EXCLUDED.revision_id` guard rests on. Spelled as a `NOT EXISTS` rather than
    /// `= (SELECT id … ORDER BY created_at DESC LIMIT 1)`: over 10,000 parts the sort node that second
    /// form adds per row cost 29 ms of a 52 ms answer, and this one costs 6 ms of 29 ms.
    ///
    /// A descriptor of the wrong length is left out rather than refused: the table's check makes that
    /// unreachable, and a corrupt row should cost a re-profile, not a 500 on every likeness read in
    /// the library.
    pub async fn shapes(&self, library: LibraryId) -> Result<Vec<Shaped>, DbError> {
        let rows: Vec<(Uuid, f64, Vec<f32>)> = sqlx::query_as(
            "SELECT ps.part_id, ps.size_mm, ps.descriptor \
               FROM part_shape ps \
               JOIN part p ON p.id = ps.part_id AND p.deleted_at IS NULL \
              WHERE ps.library_id = $1 AND ps.version = $2 \
                AND NOT EXISTS (SELECT 1 FROM revision n \
                                 WHERE n.part_id = ps.part_id AND n.id > ps.revision_id) \
              ORDER BY ps.size_mm, ps.part_id",
        )
        .bind(library.as_uuid())
        .bind(SHAPE_VERSION)
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(part, size_mm, descriptor)| {
                Some(Shaped {
                    part: PartId::from_uuid(part),
                    profile: ShapeProfile {
                        size_mm,
                        descriptor: <[f32; DESCRIPTOR_LEN]>::try_from(descriptor).ok()?,
                    },
                })
            })
            .collect())
    }

    /// Every live part of this library, with the hash of its current source file.
    ///
    /// A part with no revision is left out, exactly as the grid leaves it out: it has no bytes and no
    /// shape, so it is neither a duplicate nor a part waiting to be profiled.
    pub async fn parts(&self, library: LibraryId) -> Result<Vec<LivePart>, DbError> {
        let rows: Vec<(Uuid, i64, Option<String>)> = sqlx::query_as(
            "SELECT p.id, (extract(epoch FROM p.created_at) * 1000000)::bigint, s.blake3 \
               FROM part p \
               JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                             ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
               LEFT JOIN LATERAL (SELECT blake3 FROM file WHERE revision_id = r.id \
                                  AND role = 'source' ORDER BY created_at DESC, id DESC LIMIT 1) \
                                 s ON true \
              WHERE p.library_id = $1 AND p.deleted_at IS NULL",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(|(part, created_us, source)| {
                Ok(LivePart {
                    part: PartId::from_uuid(part),
                    created_at: detail_stamp("part.created_at", created_us)?,
                    source: source
                        .map(|hex| {
                            BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                                column: "file.blake3",
                                value: hex,
                            })
                        })
                        .transpose()?,
                })
            })
            .collect()
    }

    /// The live parts of `library`, other than `part`, whose current source file has the same hash as
    /// `part`'s — the same bytes, which needs no profile. Empty when `part` has no source file.
    ///
    /// The hash is found here rather than passed in, so there is one definition of "this part's current
    /// bytes" and the caller cannot hand in a hash it read from somewhere staler. Driven off
    /// `file_blake3`, so it costs nothing on a library where no two parts share a file.
    pub async fn identical(
        &self,
        library: LibraryId,
        part: PartId,
    ) -> Result<Vec<PartId>, DbError> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "WITH mine AS (SELECT f.blake3 FROM revision r \
                              JOIN file f ON f.revision_id = r.id AND f.role = 'source' \
                             WHERE r.part_id = $2 \
                             ORDER BY r.created_at DESC, r.id DESC, f.created_at DESC, f.id DESC \
                             LIMIT 1) \
             SELECT DISTINCT p.id FROM file f \
               JOIN revision r ON r.id = f.revision_id \
               JOIN part p ON p.id = r.part_id \
              WHERE f.role = 'source' AND f.blake3 = (SELECT blake3 FROM mine) \
                AND p.library_id = $1 AND p.deleted_at IS NULL AND p.id <> $2 \
                AND r.id = (SELECT id FROM revision WHERE part_id = p.id \
                            ORDER BY created_at DESC, id DESC LIMIT 1)",
        )
        .bind(library.as_uuid())
        .bind(part.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(ids.into_iter().map(PartId::from_uuid).collect())
    }

    /// This library's `part_link` rows, or only the ones naming `of`. A kind this build does not know
    /// is left out, so an older binary reading a newer table proposes a pair again rather than
    /// refusing the whole read.
    pub async fn links(
        &self,
        library: LibraryId,
        of: Option<PartId>,
    ) -> Result<Vec<LinkRow>, DbError> {
        let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
            "SELECT part_id, other_id, kind FROM part_link \
              WHERE library_id = $1 \
                AND ($2::uuid IS NULL OR part_id = $2 OR other_id = $2)",
        )
        .bind(library.as_uuid())
        .bind(of.map(|p| p.as_uuid()))
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(part, other, kind)| {
                Some(LinkRow {
                    part: PartId::from_uuid(part),
                    other: PartId::from_uuid(other),
                    kind: PartLinkKind::parse(&kind)?,
                })
            })
            .collect())
    }

    /// Record what somebody decided about a pair: `variant` or `variant`'s opposite, `distinct`.
    /// `false` when either part is not a live part of `library`, which the route answers `404`.
    ///
    /// Stored with the smaller id first, as `0048`'s check insists, so one pair is one row whichever
    /// way round it was decided. Any earlier row for the pair is replaced — including a `folded_into`
    /// one left behind by a fold that was restored, which is no longer true once somebody says these
    /// two are variants.
    pub async fn set_link(
        &self,
        library: LibraryId,
        part: PartId,
        other: PartId,
        kind: PartLinkKind,
    ) -> Result<bool, DbError> {
        let mut tx = self.0.begin().await?;
        if !both_live(&mut tx, library, part, other).await? {
            return Ok(false);
        }
        let (first, second) = ordered(part, other);
        clear_pair(&mut tx, part, other).await?;
        sqlx::query(
            "INSERT INTO part_link (part_id, other_id, library_id, kind) VALUES ($1, $2, $3, $4)",
        )
        .bind(first.as_uuid())
        .bind(second.as_uuid())
        .bind(library.as_uuid())
        .bind(kind.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Undo a `variant` or `distinct` decision, so the pair can be proposed again. `false` when there
    /// was no such decision.
    ///
    /// A `folded_into` row is deliberately not removable here: it records a removal, and the way back
    /// from a fold is Restore.
    pub async fn unlink(&self, part: PartId, other: PartId) -> Result<bool, DbError> {
        let (first, second) = ordered(part, other);
        let done = sqlx::query(
            "DELETE FROM part_link WHERE part_id = $1 AND other_id = $2 \
               AND kind IN ('variant', 'distinct')",
        )
        .bind(first.as_uuid())
        .bind(second.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Fold `part` into `into`: in one transaction, remove `part` by the same rule as any removal
    /// (`PgParts::soft_delete`'s single `deleted_at` write) and record which part was kept. Nothing is
    /// moved and nothing is deleted; Restore brings it back.
    ///
    /// The removal and the link carry the same stamp — `now()` is the transaction's — and
    /// [`PgLikeness::folds`] reads that equality. So a part folded, restored, then removed the ordinary
    /// way stops being a fold, which is the truth: the link no longer describes why it is gone.
    pub async fn fold(&self, part: PartId, into: PartId) -> Result<Folded, DbError> {
        if part == into {
            return Ok(Folded::IntoItself);
        }
        let mut tx = self.0.begin().await?;
        let library: Option<Uuid> = sqlx::query_scalar(
            "SELECT a.library_id FROM part a JOIN part b ON b.library_id = a.library_id \
              WHERE a.id = $1 AND b.id = $2 \
                AND a.deleted_at IS NULL AND b.deleted_at IS NULL FOR SHARE",
        )
        .bind(part.as_uuid())
        .bind(into.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(library) = library else {
            return Ok(Folded::NoSuchPair);
        };
        clear_pair(&mut tx, part, into).await?;
        let removed =
            sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
                .bind(part.as_uuid())
                .execute(&mut *tx)
                .await?;
        if removed.rows_affected() != 1 {
            return Ok(Folded::NoSuchPair);
        }
        sqlx::query(
            "INSERT INTO part_link (part_id, other_id, library_id, kind) \
             VALUES ($1, $2, $3, 'folded_into')",
        )
        .bind(part.as_uuid())
        .bind(into.as_uuid())
        .bind(library)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Folded::Done)
    }

    /// The folds this library holds, newest first: the removed part, and the part it was folded into.
    ///
    /// Only a part still removed *by that fold* — `part.deleted_at` is the link's own stamp. A part
    /// restored since is an ordinary live part and a part removed again by hand is an ordinary removed
    /// one; neither was folded into anything any more.
    pub async fn folds(&self, library: LibraryId) -> Result<Vec<(PartId, PartId)>, DbError> {
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT pl.part_id, pl.other_id FROM part_link pl \
               JOIN part p ON p.id = pl.part_id \
              WHERE pl.library_id = $1 AND pl.kind = 'folded_into' \
                AND p.deleted_at = pl.created_at \
              ORDER BY pl.created_at DESC, pl.part_id",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(part, into)| (PartId::from_uuid(part), PartId::from_uuid(into)))
            .collect())
    }
}

/// The pair as `0048`'s `part_id < other_id` check wants it.
fn ordered(part: PartId, other: PartId) -> (PartId, PartId) {
    if part.as_uuid() < other.as_uuid() {
        (part, other)
    } else {
        (other, part)
    }
}

/// Whether both are live parts of `library`, held for the rest of the transaction.
async fn both_live(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library: LibraryId,
    part: PartId,
    other: PartId,
) -> Result<bool, DbError> {
    let held: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM part \
          WHERE id = ANY($1::uuid[]) AND library_id = $2 AND deleted_at IS NULL FOR SHARE",
    )
    .bind(vec![part.as_uuid(), other.as_uuid()])
    .bind(library.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    Ok(held.len() == 2)
}

/// Drop whatever this pair was decided to be, either way round, so the row about to be written is the
/// only one about it.
async fn clear_pair(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    part: PartId,
    other: PartId,
) -> Result<(), DbError> {
    sqlx::query("DELETE FROM part_link WHERE (part_id, other_id) IN (($1, $2), ($2, $1))")
        .bind(part.as_uuid())
        .bind(other.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// What a part's page lists beside the parts it shares bytes with: parts alike in shape and size, and
/// then parts merely alike in shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Alike {
    pub near_duplicates: Vec<PartId>,
    pub similar: Vec<PartId>,
}

/// Rank a library's profiles against one part's, closest first.
///
/// `listed` is what the caller already shows elsewhere — the part itself, the parts identical to it,
/// its variants — and is left out of both lists. `decided` is the parts somebody said are not the same
/// as this one: out of `near_duplicates`, because "never propose again" is what `distinct` means, and
/// still in `similar`, because "more like this" is a way to browse and not a proposal.
///
/// Ties break on id, so a page reloaded twice reads the same way.
pub fn alike(
    subject: &ShapeProfile,
    shapes: &[Shaped],
    listed: &HashSet<PartId>,
    decided: &HashSet<PartId>,
    limit: usize,
) -> Alike {
    let mut ranked: Vec<(f32, bool, PartId)> = shapes
        .iter()
        .filter(|row| !listed.contains(&row.part))
        .map(|row| {
            (
                distance(&subject.descriptor, &row.profile.descriptor),
                is_near_duplicate(subject, &row.profile) && !decided.contains(&row.part),
                row.part,
            )
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.2.as_uuid().cmp(&b.2.as_uuid()))
    });
    Alike {
        near_duplicates: ranked
            .iter()
            .filter(|(_, near, _)| *near)
            .map(|(_, _, part)| *part)
            .take(limit)
            .collect(),
        similar: ranked
            .iter()
            .filter(|(_, near, _)| !*near)
            .map(|(_, _, part)| *part)
            .take(limit)
            .collect(),
    }
}

/// One group of parts that look like duplicates of each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    /// Two or more, largest `size_mm` first.
    pub parts: Vec<PartId>,
    /// Every part in the group came from the same bytes, so no shape judgement went into it.
    pub identical: bool,
}

/// A library's duplicate clusters, computed from what [`PgLikeness::parts`] and
/// [`PgLikeness::shapes`] read. Never stored: a decision changes the answer, and so does one part's
/// next revision.
///
/// Two parts are joined when they came from the same bytes, or when they are near-duplicates by
/// [`is_near_duplicate`] — and in both cases only while nobody has decided about the pair. Joining is
/// transitive: A like B and B like C is one group of three, because presenting it as two overlapping
/// pairs asks a person the same question twice.
///
/// `shapes` must be ordered by `size_mm` ascending, as [`PgLikeness::shapes`] returns it: the sweep
/// stops at the first candidate past the size band instead of comparing every pair.
///
/// `since` keeps only the groups holding a part created at or after it — "these new parts look like
/// parts already here".
pub fn clusters(
    parts: &[LivePart],
    shapes: &[Shaped],
    decided: &[(PartId, PartId)],
    since: Option<Timestamp>,
) -> Vec<Cluster> {
    let at: HashMap<PartId, usize> = parts
        .iter()
        .enumerate()
        .map(|(n, row)| (row.part, n))
        .collect();
    let decided: HashSet<(Uuid, Uuid)> = decided
        .iter()
        .map(|(a, b)| {
            let (first, second) = ordered(*a, *b);
            (first.as_uuid(), second.as_uuid())
        })
        .collect();
    let undecided = |a: PartId, b: PartId| {
        let (first, second) = ordered(a, b);
        !decided.contains(&(first.as_uuid(), second.as_uuid()))
    };
    let mut union = Union::of(parts.len());

    // The same bytes. Needs no profile, so this runs over every live part.
    let mut by_source: HashMap<BlobHash, Vec<usize>> = HashMap::new();
    for (n, row) in parts.iter().enumerate() {
        if let Some(source) = row.source {
            by_source.entry(source).or_default().push(n);
        }
    }
    for group in by_source.values() {
        for (offset, a) in group.iter().enumerate() {
            for b in &group[offset + 1..] {
                if undecided(parts[*a].part, parts[*b].part) {
                    union.join(*a, *b);
                }
            }
        }
    }

    // The same shape and about the same size, swept over the size-ordered profiles.
    let band = size_band();
    for (offset, a) in shapes.iter().enumerate() {
        for b in &shapes[offset + 1..] {
            if (b.profile.size_mm / a.profile.size_mm).ln() > band {
                break;
            }
            if is_near_duplicate(&a.profile, &b.profile)
                && undecided(a.part, b.part)
                && let (Some(x), Some(y)) = (at.get(&a.part), at.get(&b.part))
            {
                union.join(*x, *y);
            }
        }
    }

    let size: HashMap<PartId, f64> = shapes
        .iter()
        .map(|row| (row.part, row.profile.size_mm))
        .collect();
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for n in 0..parts.len() {
        groups.entry(union.root(n)).or_default().push(n);
    }
    let mut clusters: Vec<Cluster> = groups
        .into_values()
        .filter(|group| group.len() > 1)
        .filter(|group| {
            since.is_none_or(|since| group.iter().any(|n| parts[*n].created_at >= since))
        })
        .map(|group| {
            let mut members: Vec<PartId> = group.iter().map(|n| parts[*n].part).collect();
            members.sort_by(|a, b| {
                largest_first(size.get(a), size.get(b)).then_with(|| a.as_uuid().cmp(&b.as_uuid()))
            });
            let identical = group
                .iter()
                .all(|n| parts[*n].source.is_some() && parts[*n].source == parts[group[0]].source);
            Cluster {
                parts: members,
                identical,
            }
        })
        .collect();
    clusters.sort_by(|a, b| {
        largest_first(size.get(&a.parts[0]), size.get(&b.parts[0]))
            .then_with(|| a.parts[0].as_uuid().cmp(&b.parts[0].as_uuid()))
    });
    clusters
}

/// Bigger first, and a part with no profile — identical by bytes alone — after every part that has
/// one.
fn largest_first(a: Option<&f64>, b: Option<&f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => b.total_cmp(a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Union-find over the live parts, by index. Path-halving and union by index; no ranks, because the
/// sets here are tens of parts and the code that keeps ranks correct is more than the sets are worth.
struct Union(Vec<usize>);

impl Union {
    fn of(n: usize) -> Self {
        Union((0..n).collect())
    }

    fn root(&mut self, mut n: usize) -> usize {
        while self.0[n] != n {
            self.0[n] = self.0[self.0[n]];
            n = self.0[n];
        }
        n
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.0[a] = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(n: u8) -> PartId {
        let mut bytes = [0u8; 16];
        bytes[0] = 0x01;
        bytes[15] = n;
        PartId::from_uuid(Uuid::from_bytes(bytes))
    }

    fn profile(size_mm: f64, fill: f32) -> ShapeProfile {
        ShapeProfile {
            size_mm,
            descriptor: [fill; DESCRIPTOR_LEN],
        }
    }

    /// One bin moved by `off`, so the distance from `profile(_, fill)` is exactly `off`.
    fn nudged(size_mm: f64, fill: f32, off: f32) -> ShapeProfile {
        let mut p = profile(size_mm, fill);
        p.descriptor[0] += off;
        p
    }

    fn live(n: u8, source: u8) -> LivePart {
        LivePart {
            part: part(n),
            created_at: Timestamp::from_second(1_700_000_000 + i64::from(n)).expect("a timestamp"),
            source: Some(BlobHash::from_bytes([source; 32])),
        }
    }

    fn shaped(n: u8, profile: ShapeProfile) -> Shaped {
        Shaped {
            part: part(n),
            profile,
        }
    }

    /// `clusters` wants the profiles size-ordered; a test that builds them by hand says so here
    /// rather than relying on the order it happened to write them in.
    fn by_size(mut shapes: Vec<Shaped>) -> Vec<Shaped> {
        shapes.sort_by(|a, b| a.profile.size_mm.total_cmp(&b.profile.size_mm));
        shapes
    }

    #[test]
    fn a_scaled_copy_is_similar_and_not_a_near_duplicate() {
        let subject = profile(20.0, 0.12);
        let shapes = vec![
            // The same shape at twice the size: distance 0, well outside the size band.
            shaped(2, profile(40.0, 0.12)),
            // The same size, a hair apart in shape.
            shaped(3, nudged(20.1, 0.12, 0.01)),
        ];
        let alike = alike(&subject, &shapes, &HashSet::new(), &HashSet::new(), 24);
        assert_eq!(alike.near_duplicates, vec![part(3)]);
        assert_eq!(alike.similar, vec![part(2)]);
    }

    #[test]
    fn a_decided_pair_leaves_the_near_duplicates_and_a_listed_one_leaves_both() {
        let subject = profile(20.0, 0.12);
        let shapes = vec![
            shaped(2, nudged(20.0, 0.12, 0.01)),
            shaped(3, nudged(20.0, 0.12, 0.02)),
        ];
        let decided = HashSet::from([part(2)]);
        let ranked = alike(&subject, &shapes, &HashSet::new(), &decided, 24);
        assert_eq!(ranked.near_duplicates, vec![part(3)]);
        assert_eq!(
            ranked.similar,
            vec![part(2)],
            "not the same is still more like this"
        );

        let listed = HashSet::from([part(2)]);
        let ranked = alike(&subject, &shapes, &listed, &HashSet::new(), 24);
        assert_eq!(ranked.near_duplicates, vec![part(3)]);
        assert!(
            ranked.similar.is_empty(),
            "a listed part is on no other list"
        );
    }

    #[test]
    fn the_lists_are_closest_first_and_capped_at_the_limit() {
        let subject = profile(20.0, 0.12);
        let shapes = vec![
            shaped(4, nudged(20.0, 0.12, 0.03)),
            shaped(2, nudged(20.0, 0.12, 0.01)),
            shaped(3, nudged(20.0, 0.12, 0.02)),
        ];
        let ranked = alike(&subject, &shapes, &HashSet::new(), &HashSet::new(), 24);
        assert_eq!(ranked.near_duplicates, vec![part(2), part(3), part(4)]);
        let ranked = alike(&subject, &shapes, &HashSet::new(), &HashSet::new(), 2);
        assert_eq!(ranked.near_duplicates, vec![part(2), part(3)]);
    }

    #[test]
    fn a_and_b_and_b_and_c_are_one_cluster() {
        // Each pair 0.03 apart, A and C 0.06 apart — past the threshold on their own.
        let parts = vec![live(1, 0xa1), live(2, 0xa2), live(3, 0xa3)];
        let shapes = by_size(vec![
            shaped(1, nudged(20.0, 0.12, 0.0)),
            shaped(2, nudged(20.0, 0.12, 0.03)),
            shaped(3, nudged(20.0, 0.12, 0.06)),
        ]);
        let got = clusters(&parts, &shapes, &[], None);
        assert_eq!(got.len(), 1, "one group, not two pairs");
        assert_eq!(got[0].parts.len(), 3);
        assert!(!got[0].identical);
    }

    #[test]
    fn the_same_bytes_cluster_without_a_profile_and_say_so() {
        let parts = vec![live(1, 0xa1), live(2, 0xa1), live(3, 0xa2)];
        let got = clusters(&parts, &[], &[], None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].parts, vec![part(1), part(2)]);
        assert!(got[0].identical, "the same bytes, no shape judgement");
    }

    #[test]
    fn a_decided_pair_is_never_a_cluster() {
        let parts = vec![live(1, 0xa1), live(2, 0xa2)];
        let shapes = by_size(vec![
            shaped(1, nudged(20.0, 0.12, 0.0)),
            shaped(2, nudged(20.0, 0.12, 0.01)),
        ]);
        assert_eq!(clusters(&parts, &shapes, &[], None).len(), 1);
        assert!(
            clusters(&parts, &shapes, &[(part(2), part(1))], None).is_empty(),
            "either way round"
        );
    }

    #[test]
    fn a_scaled_copy_is_no_cluster() {
        let parts = vec![live(1, 0xa1), live(2, 0xa2)];
        let shapes = by_size(vec![
            shaped(1, profile(20.0, 0.12)),
            shaped(2, profile(40.0, 0.12)),
        ]);
        assert!(clusters(&parts, &shapes, &[], None).is_empty());
    }

    #[test]
    fn a_cluster_is_largest_first_and_since_keeps_the_new_ones() {
        let parts = vec![live(1, 0xa1), live(2, 0xa2)];
        let shapes = by_size(vec![
            shaped(1, nudged(20.0, 0.12, 0.0)),
            shaped(2, nudged(20.2, 0.12, 0.01)),
        ]);
        let got = clusters(&parts, &shapes, &[], None);
        assert_eq!(
            got[0].parts,
            vec![part(2), part(1)],
            "largest size_mm first"
        );

        let after_both = Timestamp::from_second(1_700_000_003).expect("a timestamp");
        assert!(clusters(&parts, &shapes, &[], Some(after_both)).is_empty());
        assert_eq!(
            clusters(&parts, &shapes, &[], Some(parts[1].created_at)).len(),
            1
        );
    }
}
