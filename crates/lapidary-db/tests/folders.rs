use lapidary_core::{FolderId, LibraryId};
use lapidary_db::{DbError, PgFolders};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

#[sqlx::test(migrations = "./migrations")]
async fn get_or_create_is_idempotent(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let a = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("creates");
    let b = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("finds");
    assert_eq!(a, b, "two workers racing one directory get one row");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_same_name_under_two_parents_is_two_folders(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = f
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let a = f
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("a");
    let b = f
        .get_or_create(library(), Some(bases), "Rocks", "Rocks")
        .await
        .expect("b");
    assert_ne!(a, b);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_folder_cannot_move_into_its_own_descendant(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = f
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    let cliffs = f
        .get_or_create(library(), Some(rocks), "Cliffs", "Cliffs")
        .await
        .expect("Cliffs");

    assert!(
        f.would_cycle(terrain, cliffs).await.expect("checks"),
        "into a descendant"
    );
    assert!(
        !f.would_cycle(cliffs, terrain).await.expect("checks"),
        "the legal direction"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn slug_path_joins_the_ancestors(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = f
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    assert_eq!(f.slug_path(rocks).await.expect("path"), "Terrain/Rocks");
}

#[sqlx::test(migrations = "./migrations")]
async fn deleting_a_folder_cascades_through_subfolders(pool: sqlx::PgPool) {
    // The one-level bug passes every other test in this file.
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = f
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    let _cliffs = f
        .get_or_create(library(), Some(rocks), "Cliffs", "Cliffs")
        .await
        .expect("Cliffs");

    let (folders, _parts) = f.soft_delete_subtree(terrain).await.expect("deletes");
    assert_eq!(folders, 3, "Terrain, Rocks and Cliffs — not just Terrain");
    assert!(
        f.tree(library()).await.expect("tree").is_empty(),
        "all hidden"
    );
}

// Rider: self-drop is the first thing a drag-and-drop tree produces.
#[sqlx::test(migrations = "./migrations")]
async fn would_cycle_of_a_folder_into_itself_is_true(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    assert!(
        f.would_cycle(terrain, terrain).await.expect("checks"),
        "a folder cannot become its own parent"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn reparent_moves_a_folder_and_slug_path_follows_it(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = f
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let rocks = f
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");

    assert!(
        f.reparent(rocks, Some(bases)).await.expect("moves"),
        "the row exists, so the move must match"
    );
    assert_eq!(
        f.slug_path(rocks).await.expect("path"),
        "Bases/Rocks",
        "slug_path must follow the move, not the old parent"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn rename_changes_the_name_and_slug_path_reflects_it(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = f
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");

    assert!(
        f.rename(rocks, "Cliffs", "Cliffs").await.expect("renames"),
        "the row exists, so the rename must match"
    );
    assert_eq!(
        f.slug_path(rocks).await.expect("path"),
        "Terrain/Cliffs",
        "slug_path must reflect the new slug, not the old one"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn library_of_names_the_right_library_and_none_for_an_unknown_id(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");

    assert_eq!(
        f.library_of(terrain).await.expect("looks up"),
        Some(library())
    );
    assert_eq!(
        f.library_of(FolderId::new()).await.expect("looks up"),
        None,
        "a folder nobody created answers None, not an error"
    );
}

/// The race `reparent`'s advisory lock exists to close: two folders, siblings at the
/// library root, each trying to become the other's child at the same time. A's read of
/// `would_cycle == false` and B's read of the same are each individually correct at the
/// instant they run — the cycle only exists once both writes land. Without an atomic
/// guard, both can commit.
///
/// Genuinely concurrent, not sequential-dressed-as-concurrent: `tokio::join!` polls both
/// `reparent` futures together, each holds its own pool connection
/// (`PgFolders(pool.clone())` per side, so neither borrows the other's connection), and
/// each does real network round-trips to Postgres before either commits. Whichever
/// transaction loses the race for `pg_advisory_xact_lock` blocks inside Postgres until the
/// winner commits, then re-runs its ancestry check against the now-committed state and
/// finds the cycle the winner just created — so the outcome (exactly one success, one
/// `WouldCreateCycle`) is deterministic, but *which side* wins is not, and this test does
/// not assume one.
#[sqlx::test(migrations = "./migrations")]
async fn two_folders_swapping_parents_at_once_produce_exactly_one_cycle_refusal(
    pool: sqlx::PgPool,
) {
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = f
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");

    let a = PgFolders(pool.clone());
    let b = PgFolders(pool.clone());
    let (terrain_under_bases, bases_under_terrain) = tokio::join!(
        a.reparent(terrain, Some(bases)),
        b.reparent(bases, Some(terrain)),
    );

    let outcomes = [&terrain_under_bases, &bases_under_terrain];
    let successes = outcomes.iter().filter(|r| matches!(r, Ok(true))).count();
    let refusals = outcomes
        .iter()
        .filter(|r| matches!(r, Err(DbError::WouldCreateCycle { .. })))
        .count();
    assert_eq!(
        successes, 1,
        "exactly one side of the swap must land, got {terrain_under_bases:?} / {bases_under_terrain:?}"
    );
    assert_eq!(
        refusals, 1,
        "the other side must be refused as a cycle, got {terrain_under_bases:?} / {bases_under_terrain:?}"
    );

    // Not just "one call failed" — the tree itself must have exactly one edge between the
    // two, never both (a cycle) and never neither (a lost update).
    let rows = f.tree(library()).await.expect("tree");
    let parent_of = |id: FolderId| {
        rows.iter()
            .find(|r| r.id == id)
            .expect("row exists")
            .parent_id
    };
    let (terrain_parent, bases_parent) = (parent_of(terrain), parent_of(bases));
    assert!(
        (terrain_parent == Some(bases) && bases_parent.is_none())
            || (bases_parent == Some(terrain) && terrain_parent.is_none()),
        "exactly one parent-child edge must exist after the race, got terrain_parent={terrain_parent:?} bases_parent={bases_parent:?}"
    );
}

/// A chain one level deeper than the walk cap that used to bound every query in
/// `folders.rs`. Seventeen categories, each parented on the last, returned root first.
///
/// Built through `get_or_create` rather than raw SQL because nothing refuses it: no route
/// bounds how deep a category can be nested, which is the whole reason a tree this shape
/// is reachable at all.
async fn deep_chain(pool: &sqlx::PgPool, levels: usize) -> Vec<FolderId> {
    let f = PgFolders(pool.clone());
    let mut chain: Vec<FolderId> = Vec::with_capacity(levels);
    for level in 0..levels {
        let name = format!("Level {level:02}");
        let parent = chain.last().copied();
        chain.push(
            f.get_or_create(library(), parent, &name, &name)
                .await
                .expect("creates one level of the chain"),
        );
    }
    chain
}

#[sqlx::test(migrations = "./migrations")]
async fn a_chain_deeper_than_sixteen_still_refuses_the_cycle_that_would_close_it(
    pool: sqlx::PgPool,
) {
    // The boundary the old `depth < 16` bound left open. Walking up from the seventeenth
    // folder, the sixteen rows a capped walk sees are the folder itself and fifteen
    // ancestors — the root, the one being moved, is the one it never reaches. So the
    // ancestry check answered "no cycle" and the UPDATE landed, and the tree closed into a
    // real loop that the foreign key and `folder_library_parent` both accept.
    let chain = deep_chain(&pool, 17).await;
    let (root, leaf) = (chain[0], chain[16]);
    let f = PgFolders(pool.clone());

    assert!(
        f.would_cycle(root, leaf).await.expect("checks"),
        "the root is an ancestor of the seventeenth folder, however far up that is"
    );
    assert!(
        matches!(
            f.reparent(root, Some(leaf)).await,
            Err(DbError::WouldCreateCycle { .. })
        ),
        "and the write refuses it too — the in-lock check is the one that decides"
    );

    // The legal direction still works at this depth, so the refusal above is a refusal and
    // not a walk that gave up.
    assert!(
        !f.would_cycle(leaf, root).await.expect("checks"),
        "moving the leaf under the root it already descends from is not a cycle"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn slug_path_at_seventeen_levels_still_starts_at_the_library_root(pool: sqlx::PgPool) {
    // A truncated path is worse than an error: it names a directory that is a sibling of
    // the real root, so the next model ingested under this category is written outside the
    // tree its category lives in, silently.
    let chain = deep_chain(&pool, 17).await;
    let path = PgFolders(pool)
        .slug_path(chain[16])
        .await
        .expect("slug path");
    assert_eq!(
        path.split('/').count(),
        17,
        "every level, root first — got {path}"
    );
    assert!(
        path.starts_with("Level 00/Level 01/"),
        "rooted — got {path}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn deleting_a_deep_category_hides_the_models_below_the_old_walk_cap(pool: sqlx::PgPool) {
    // The count this returns is the number the confirmation dialog shows. A part left
    // undeleted under a category that has vanished is that dialog understating what it
    // just did, on the one screen where the promise is that nothing is lost.
    let chain = deep_chain(&pool, 17).await;
    sqlx::query(
        "INSERT INTO part (id, library_id, name, source_path, folder_id) \
         VALUES (gen_random_uuid(), $1, 'Cliff face, LP-7712-04', \
                 'level-00/.../cliff-face-lp-7712-04.stl', $2)",
    )
    .bind(library().as_uuid())
    .bind(chain[16].as_uuid())
    .execute(&pool)
    .await
    .expect("a model filed under the deepest category");

    let (folders, parts) = PgFolders(pool)
        .soft_delete_subtree(chain[0])
        .await
        .expect("deletes");
    assert_eq!(folders, 17, "every category under the one deleted");
    assert_eq!(parts, 1, "and the model filed under the deepest of them");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_part_in_another_library_is_not_counted_under_this_librarys_category(pool: sqlx::PgPool) {
    // Correct today only by luck. Every writer of `part.folder_id` -- the scan, the move
    // route, migration `0008`'s backfill -- keeps a part and its folder in one library, so
    // the count join never had to say so. Nothing in the schema requires it: `part.folder_id`
    // references `folder(id)` and no constraint relates the two `library_id`s, so one bad
    // row from a repair script or a future writer would put a foreign model in this
    // library's sidebar count and in the number its delete confirmation shows.
    let f = PgFolders(pool.clone());
    let terrain = f
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");

    let other = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name) VALUES ($1, 'Shop floor')")
        .bind(other.as_uuid())
        .execute(&pool)
        .await
        .expect("a second library");
    sqlx::query(
        "INSERT INTO part (id, library_id, name, source_path, folder_id) \
         VALUES (gen_random_uuid(), $1, 'Impeller, LP-5501-02', \
                 'impeller-lp-5501-02.stl', $2)",
    )
    .bind(other.as_uuid())
    .bind(terrain.as_uuid())
    .execute(&pool)
    .await
    .expect("a part in the other library, filed under this library's category");

    let rows = f.tree(library()).await.expect("tree");
    let terrain_row = rows
        .iter()
        .find(|r| r.id == terrain)
        .expect("Terrain is in the tree");
    assert_eq!(
        terrain_row.part_count, 0,
        "the count beside a category is the number of cards this library's grid shows \
         for it, and the grid filters on library"
    );
}
