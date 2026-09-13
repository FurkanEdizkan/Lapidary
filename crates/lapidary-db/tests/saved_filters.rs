//! Saved filters: a name for a set of the grid's filters, kept per library.

use lapidary_core::LibraryId;
use lapidary_db::{DbError, PgParts, PgSavedFilters};
use serde_json::json;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

#[sqlx::test(migrations = "./migrations")]
async fn saved_filters_are_listed_by_name_with_the_filters_they_keep(pool: sqlx::PgPool) {
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
    assert_eq!(names, ["Brackets in S235JR", "Stock STL"]);
    assert_eq!(listed[0].id, brackets);
    assert_eq!(listed[1].id, stock);
    let search: serde_json::Value = serde_json::from_str(&listed[1].search).expect("JSON");
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
        .create_library("Workshop fixtures", "hobby")
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
        .create_library("Workshop fixtures", "hobby")
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
