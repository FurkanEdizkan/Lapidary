//! `PgLocks`: one active check-out per part, in a controlled library only, and a record of
//! how each one ended.

use lapidary_core::{BlobHash, LibraryId, LockId, MeshMeasurements, PartId};
use lapidary_db::{Checkout, IngestRequest, PgIngest, PgLocks, PgParts, StoredBlobRow};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

async fn seed(pool: &sqlx::PgPool) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Flange DN40, LP-3310-02",
            source_path: "flange-dn40-lp-3310-02.stl",
            folder: None,
            storage_path: Some(
                "libraries/default/flange-dn40-lp-3310-02/flange-dn40-lp-3310-02.stl",
            ),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([0x51; 32]),
                size_bytes: 184_342,
                stored_bytes: 184_342,
                zstd_level: 0,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [150.0, 150.0, 18.0],
                triangle_count: 36_868,
                surface_area_mm2: 41_210.5,
                volume_mm3: Some(214_780.0),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeds the part")
}

async fn controlled(pool: &sqlx::PgPool) -> PartId {
    assert!(
        PgParts(pool.clone())
            .make_controlled(library())
            .await
            .expect("switches the library")
    );
    seed(pool).await
}

#[sqlx::test(migrations = "./migrations")]
async fn a_part_is_checked_out_once_and_the_next_asker_is_told_by_whom(pool: sqlx::PgPool) {
    let part = controlled(&pool).await;
    let locks = PgLocks(pool.clone());
    let Checkout::Taken(first) = locks.take(part, "mira@workshop-pc").await.expect("takes") else {
        panic!("the first check-out is taken");
    };
    assert_eq!(first.holder, "mira@workshop-pc");

    match locks.take(part, "jonas@laptop").await.expect("asks") {
        Checkout::Held(held) => assert_eq!(held, first, "the second asker is told who holds it"),
        other => panic!("a checked-out part is not checked out twice: {other:?}"),
    }
    assert_eq!(locks.active(part).await.expect("reads"), Some(first));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_hobby_library_and_a_missing_part_have_nothing_to_check_out(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let locks = PgLocks(pool.clone());
    assert_eq!(
        locks.take(part, "mira@workshop-pc").await.expect("asks"),
        Checkout::HobbyLibrary
    );
    assert_eq!(
        locks
            .take(PartId::new(), "mira@workshop-pc")
            .await
            .expect("asks"),
        Checkout::NoSuchPart
    );
    assert_eq!(locks.active(part).await.expect("reads"), None);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_the_held_lock_checks_in_and_then_the_part_is_free(pool: sqlx::PgPool) {
    let part = controlled(&pool).await;
    let locks = PgLocks(pool.clone());
    let Checkout::Taken(lock) = locks.take(part, "mira@workshop-pc").await.expect("takes") else {
        panic!("the check-out is taken");
    };

    assert!(
        !locks.check_in(part, LockId::new()).await.expect("asks"),
        "a lock this part never had checks nothing in"
    );
    assert!(locks.check_in(part, lock.id).await.expect("checks in"));
    assert!(
        !locks.check_in(part, lock.id).await.expect("asks"),
        "and not twice"
    );
    assert_eq!(locks.active(part).await.expect("reads"), None);
    assert!(matches!(
        locks.take(part, "jonas@laptop").await.expect("takes"),
        Checkout::Taken(_)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_forced_release_is_recorded_as_forced_and_by_whom(pool: sqlx::PgPool) {
    let part = controlled(&pool).await;
    let locks = PgLocks(pool.clone());
    let Checkout::Taken(lock) = locks.take(part, "mira@workshop-pc").await.expect("takes") else {
        panic!("the check-out is taken");
    };

    assert_eq!(
        locks
            .force_release(part, "jonas@laptop")
            .await
            .expect("releases"),
        Some(lock)
    );
    let (forced, released_by): (bool, String) =
        sqlx::query_as("SELECT forced, released_by FROM part_lock WHERE part_id = $1")
            .bind(part.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("the record");
    assert_eq!((forced, released_by.as_str()), (true, "jonas@laptop"));
    assert_eq!(
        locks
            .force_release(part, "jonas@laptop")
            .await
            .expect("asks"),
        None,
        "nothing is left to release"
    );
}
