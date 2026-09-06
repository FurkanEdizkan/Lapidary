use lapidary_core::LibraryId;
use lapidary_db::PgFolders;

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
