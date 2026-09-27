//! A library's tags as an index: every tag with how many parts carry it, and the tags most often on
//! the same parts as one of them (`docs/goals/P3.md`).
//!
//! **There is no tag table.** Tags live in `part.tags`, a `text[]` with a GIN index (`0022`), and an
//! index is a read over it — so a tag exists exactly as long as a live part carries it and nothing has
//! to be kept in step.
//!
//! Not `PgParts::tag_facet`, which is the grid's filter panel: that one counts **within the grid**,
//! narrowed by the format, material, category and query a person has chosen, and past
//! [`crate::EXACT_FACET_ROWS`] it withholds the counts (`docs/DATA.md` §3.4). A tag index wants the
//! whole library and cannot drop the counts, because the count is what it is ordered by.

use crate::DbError;
use lapidary_core::LibraryId;
use sqlx::PgPool;

pub struct PgTags(pub PgPool);

impl PgTags {
    /// Every tag on a live part in the library, with how many parts carry it: most-carried first, then
    /// alphabetically. A tag on no live part is simply absent — there is nowhere for it to be recorded.
    ///
    /// `count(DISTINCT p.id)`, as `material_facet` counts: `PUT /api/parts/{id}/tags` drops repeats, but a
    /// row that arrived through a mirror or a bundle import went through no such cleaning, and a tag
    /// written twice on one part must not count that part twice.
    ///
    /// No `LIMIT` and no count threshold. P3's "every tag in a library is reachable from `/tags`" is
    /// the page's whole claim, and an order by count is a lie without counts.
    //
    // ponytail: one sequential scan of the library's parts per load — `unnest` cannot use the GIN
    // index, exactly as the three facet queries cannot. `docs/DATA.md` §3.4's rollup table refreshed
    // on ingest is the upgrade, when a large library measures slow.
    pub async fn index(&self, library: LibraryId) -> Result<Vec<(String, i64)>, DbError> {
        Ok(sqlx::query_as(
            "SELECT t.tag, count(DISTINCT p.id) FROM part p \
             CROSS JOIN LATERAL unnest(p.tags) AS t(tag) \
             WHERE p.library_id = $1 AND p.deleted_at IS NULL \
             GROUP BY t.tag ORDER BY count(DISTINCT p.id) DESC, t.tag",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?)
    }

    /// How many live parts carry `tag`, and the other tags on those same parts — most shared first,
    /// then alphabetically, keeping only those sharing at least `floor` parts and at most `limit` of them.
    ///
    /// Two statements rather than one. A single query would have to carry the part count on every
    /// related row, and a tag with nothing related returns no rows at all — which is exactly the case
    /// the count is needed for, since `0` is how a page knows the tag is gone rather than lonely.
    ///
    /// `tags @> array[$2]` is the GIN index's own operator, so the set of parts is found through the
    /// index; only the unnest over that set is a scan.
    pub async fn related(
        &self,
        library: LibraryId,
        tag: &str,
        floor: i64,
        limit: i64,
    ) -> Result<(i64, Vec<(String, i64)>), DbError> {
        let parts: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM part \
             WHERE library_id = $1 AND deleted_at IS NULL AND tags @> ARRAY[$2::text]",
        )
        .bind(library.as_uuid())
        .bind(tag)
        .fetch_one(&self.0)
        .await?;
        let related: Vec<(String, i64)> = sqlx::query_as(
            "WITH tagged AS ( \
             SELECT p.id, p.tags FROM part p \
             WHERE p.library_id = $1 AND p.deleted_at IS NULL AND p.tags @> ARRAY[$2::text]) \
             SELECT o.tag, count(DISTINCT tagged.id) FROM tagged \
             CROSS JOIN LATERAL unnest(tagged.tags) AS o(tag) \
             WHERE o.tag <> $2 \
             GROUP BY o.tag HAVING count(DISTINCT tagged.id) >= $3 \
             ORDER BY count(DISTINCT tagged.id) DESC, o.tag LIMIT $4",
        )
        .bind(library.as_uuid())
        .bind(tag)
        .bind(floor)
        .bind(limit)
        .fetch_all(&self.0)
        .await?;
        Ok((parts.0, related))
    }
}
