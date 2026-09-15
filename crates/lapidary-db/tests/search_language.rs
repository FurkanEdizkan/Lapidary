//! A library's search language (`docs/DATA.md` §3.3): each part is indexed with its library's
//! configuration, and every query is built with it.

use lapidary_core::{BlobHash, LibraryId, PartId};
use lapidary_db::{
    GridQuery, IngestRequest, PartRepository, PgIngest, PgParts, Shows, StoredBlobRow,
};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// "Şaft yatağı kapağı", a shaft bearing's cover: it holds ğ, ş and ı.
const COVER: &str = "Şaft yatağı kapağı";

async fn part(pool: &sqlx::PgPool, library: LibraryId, seed: u8) -> PartId {
    let storage_path = format!("libraries/{seed}/saft-yatagi-kapagi/saft-yatagi-kapagi.stl");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library,
            name: COVER,
            source_path: "saft-yatagi-kapagi.stl",
            folder: None,
            storage_path: Some(&storage_path),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 64_120,
                stored_bytes: 0,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [72.0, 72.0, 9.5],
                triangle_count: 3_812,
                surface_area_mm2: 10_204.0,
                volume_mm3: Some(31_880.0),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("records a part")
}

fn ids(rows: Vec<lapidary_db::PartRow>) -> Vec<PartId> {
    rows.into_iter().map(|row| row.summary.id).collect()
}

/// Neither query below is a substring of the name, so the `ILIKE` half of search cannot answer for
/// the stemmer. The inflected one needs the query stemmed too: under a `simple` query it keeps its
/// suffixes and matches nothing, whatever the name was indexed with.
#[sqlx::test(migrations = "./migrations")]
async fn a_turkish_library_finds_a_part_by_other_forms_of_the_words_in_its_name(
    pool: sqlx::PgPool,
) {
    let parts = PgParts(pool.clone());
    let atolye = parts
        .create_library("Atölye fikstürleri", "hobby", "turkish")
        .await
        .expect("a Turkish library");
    let cover = part(&pool, atolye, 0x51).await;
    part(&pool, seeded(), 0x52).await;

    for query in ["yatak kapak", "yataklar kapakları"] {
        let found = parts
            .search(&GridQuery::new(atolye, 50), query)
            .await
            .expect("searches");
        assert_eq!(ids(found), [cover], "{query} in the Turkish library");
        let found = parts
            .search(&GridQuery::new(seeded(), 50), query)
            .await
            .expect("searches");
        assert!(
            ids(found).is_empty(),
            "{query} in a simple library, which stems nothing"
        );
    }

    let config: String = sqlx::query_scalar("SELECT search_config::text FROM part WHERE id = $1")
        .bind(cover.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("reads");
    assert_eq!(
        config, "turkish",
        "the part took its library's language as it was made"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_turkish_librarys_facets_count_what_its_search_finds(pool: sqlx::PgPool) {
    let parts = PgParts(pool.clone());
    let atolye = parts
        .create_library("Atölye fikstürleri", "hobby", "turkish")
        .await
        .expect("a Turkish library");
    part(&pool, atolye, 0x51).await;

    let formats = parts
        .format_facet(
            atolye,
            None,
            Some("yataklar kapakları"),
            Shows::Live,
            None,
            None,
            None,
        )
        .await
        .expect("facets");
    assert_eq!(
        formats.iter().filter_map(|value| value.count).sum::<u64>(),
        1
    );
}
