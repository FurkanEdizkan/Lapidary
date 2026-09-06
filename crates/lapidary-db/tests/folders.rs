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
