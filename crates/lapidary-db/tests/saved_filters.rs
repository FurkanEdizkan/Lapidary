//! Saved filters: a name for a set of the grid's filters, kept per library.

use lapidary_core::LibraryId;
use lapidary_db::{DbError, FilterMove, PgFolders, PgParts, PgSavedFilters};
use serde_json::json;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

#[sqlx::test(migrations = "./migrations")]
async fn saved_filters_are_listed_in_the_order_they_were_saved(pool: sqlx::PgPool) {
    let filters = PgSavedFilters(pool.clone());
    let stock = filters
        .create(
            library(),
            "Stock STL",
            &json!({ "format": "stl", "tag": "stock" }),
        )
        .await
        .expect("saves");
    let brackets = filters
        .create(
            library(),
            "Brackets in S235JR",
            &json!({ "q": "bracket", "material": "S235JR" }),
        )
        .await
        .expect("saves");

    let listed = filters.list(library()).await.expect("lists");
    let names: Vec<_> = listed.iter().map(|filter| filter.name.as_str()).collect();
    assert_eq!(
        names,
        ["Stock STL", "Brackets in S235JR"],
        "a new filter goes last"
    );
    assert_eq!(listed[0].id, stock);
    assert_eq!(listed[1].id, brackets);
    let search: serde_json::Value = serde_json::from_str(&listed[0].search).expect("JSON");
    assert_eq!(search, json!({ "format": "stl", "tag": "stock" }));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_name_is_taken_once_per_library_and_free_in_another(pool: sqlx::PgPool) {
    let filters = PgSavedFilters(pool.clone());
    filters
        .create(library(), "Stock STL", &json!({ "format": "stl" }))
        .await
        .expect("saves");
    let again = filters
        .create(library(), "Stock STL", &json!({ "format": "3mf" }))
        .await;
    assert!(
        matches!(again, Err(DbError::SavedFilterNameTaken { .. })),
        "{again:?}"
    );

    let workshop = PgParts(pool.clone())
        .create_library("Workshop fixtures", "hobby", "simple")
        .await
        .expect("a second library");
    filters
        .create(workshop, "Stock STL", &json!({ "format": "stl" }))
        .await
        .expect("another library's names are its own");
    assert_eq!(filters.list(workshop).await.expect("lists").len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_filter_for_a_library_that_does_not_exist_is_refused(pool: sqlx::PgPool) {
    let nowhere = LibraryId::new();
    let saved = PgSavedFilters(pool)
        .create(nowhere, "Stock STL", &json!({ "format": "stl" }))
        .await;
    assert!(
        matches!(saved, Err(DbError::NoSuchLibrary { library }) if library == nowhere),
        "{saved:?}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn removing_a_filter_takes_it_off_its_own_library_only(pool: sqlx::PgPool) {
    let filters = PgSavedFilters(pool.clone());
    let stock = filters
        .create(library(), "Stock STL", &json!({ "format": "stl" }))
        .await
        .expect("saves");
    let workshop = PgParts(pool.clone())
        .create_library("Workshop fixtures", "hobby", "simple")
        .await
        .expect("a second library");

    assert!(
        !filters.remove(workshop, stock).await.expect("runs"),
        "another library's id cannot remove it"
    );
    assert!(filters.remove(library(), stock).await.expect("runs"));
    assert!(
        !filters.remove(library(), stock).await.expect("runs"),
        "already gone"
    );
    assert!(filters.list(library()).await.expect("lists").is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_saved_filter_is_renamed_and_a_taken_name_refused(pool: sqlx::PgPool) {
    let filters = PgSavedFilters(pool.clone());
    let stock = filters
        .create(library(), "Stock STL", &json!({ "format": "stl" }))
        .await
        .expect("saves");
    filters
        .create(library(), "Spares", &json!({ "tag": "spare" }))
        .await
        .expect("saves");

    assert!(
        filters
            .rename(library(), stock, "Stock meshes")
            .await
            .expect("renames")
    );
    let taken = filters.rename(library(), stock, "Spares").await;
    assert!(
        matches!(taken, Err(DbError::SavedFilterNameTaken { .. })),
        "{taken:?}"
    );
    assert!(
        !filters
            .rename(library(), lapidary_core::SavedFilterId::new(), "Anything")
            .await
            .expect("asks"),
        "a filter this library does not hold is not renamed"
    );
    let names: Vec<String> = filters
        .list(library())
        .await
        .expect("lists")
        .into_iter()
        .map(|filter| filter.name)
        .collect();
    assert_eq!(names, ["Stock meshes", "Spares"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_saved_filter_moves_one_place_at_a_time_and_not_past_either_end(pool: sqlx::PgPool) {
    let filters = PgSavedFilters(pool.clone());
    let mut ids = Vec::new();
    for (name, format) in [
        ("Stock STL", "stl"),
        ("Printable 3MF", "3mf"),
        ("CAD STEP", "step"),
    ] {
        ids.push(
            filters
                .create(library(), name, &json!({ "format": format }))
                .await
                .expect("saves"),
        );
    }
    let order = |listed: Vec<lapidary_db::SavedFilterRow>| {
        listed
            .into_iter()
            .map(|filter| filter.name)
            .collect::<Vec<_>>()
    };

    assert!(
        filters
            .move_filter(library(), ids[2], FilterMove::Up)
            .await
            .expect("moves")
    );
    assert_eq!(
        order(filters.list(library()).await.expect("lists")),
        ["Stock STL", "CAD STEP", "Printable 3MF"]
    );

    assert!(
        filters
            .move_filter(library(), ids[0], FilterMove::Up)
            .await
            .expect("asks"),
        "the top stays put"
    );
    assert!(
        filters
            .move_filter(library(), ids[1], FilterMove::Down)
            .await
            .expect("asks"),
        "the bottom stays put"
    );
    assert_eq!(
        order(filters.list(library()).await.expect("lists")),
        ["Stock STL", "CAD STEP", "Printable 3MF"]
    );

    assert!(
        filters
            .move_filter(library(), ids[0], FilterMove::Down)
            .await
            .expect("moves")
    );
    assert_eq!(
        order(filters.list(library()).await.expect("lists")),
        ["CAD STEP", "Stock STL", "Printable 3MF"]
    );
    assert!(
        !filters
            .move_filter(
                library(),
                lapidary_core::SavedFilterId::new(),
                FilterMove::Up
            )
            .await
            .expect("asks")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_filter_whose_category_was_deleted_is_marked_and_a_restored_one_is_not(
    pool: sqlx::PgPool,
) {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("a category");
    let filters = PgSavedFilters(pool.clone());
    filters
        .create(
            library(),
            "Terrain",
            &json!({ "folderId": terrain.to_string() }),
        )
        .await
        .expect("saves");
    filters
        .create(library(), "Stock STL", &json!({ "format": "stl" }))
        .await
        .expect("saves");
    let gone = |listed: Vec<lapidary_db::SavedFilterRow>| {
        listed
            .into_iter()
            .map(|filter| (filter.name, filter.folder_gone))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        gone(filters.list(library()).await.expect("lists")),
        [
            ("Terrain".to_owned(), false),
            ("Stock STL".to_owned(), false)
        ]
    );

    folders
        .soft_delete_subtree(terrain)
        .await
        .expect("deletes the category");
    assert_eq!(
        gone(filters.list(library()).await.expect("lists")),
        [
            ("Terrain".to_owned(), true),
            ("Stock STL".to_owned(), false)
        ],
        "only the filter naming the deleted category is marked"
    );

    sqlx::query("UPDATE folder SET deleted_at = NULL WHERE id = $1")
        .bind(terrain.as_uuid())
        .execute(&pool)
        .await
        .expect("restores the category");
    assert_eq!(
        gone(filters.list(library()).await.expect("lists")),
        [
            ("Terrain".to_owned(), false),
            ("Stock STL".to_owned(), false)
        ],
        "the mark is read on every list"
    );
}
