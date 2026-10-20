//! The sign-in wall (A28): the pictures a signed-out page may show. Only the first picture of a part shared with
//! everyone, not hidden from the wall by its owner, while the admin has not switched the wall off — and nothing else
//! about the part. Both reads carry the whole condition, so the picture route can never serve what the list would not.
//! The list names each picture by its id and the picture route reads it by that id (A36), so the signed-out page
//! carries no picture's bytes inline.

use crate::DbError;
use lapidary_core::{BlobHash, PartId, PartImageId};
use sqlx::PgPool;
use uuid::Uuid;

/// The condition, once: `p` is the part, `i` its first picture.
macro_rules! on_the_wall {
    () => {
        " FROM part p \
         JOIN LATERAL (SELECT id, image_webp, blake3, fit, focus_x, focus_y FROM part_image \
                       WHERE part_id = p.id ORDER BY position, id LIMIT 1) i ON true \
         WHERE p.shared_with_everyone AND p.on_wall \
           AND coalesce((SELECT value FROM instance_setting WHERE key = 'sign_in_wall'), 'true') = 'true'"
    };
}

/// One picture on the wall: which, and how it sits in its frame.
#[derive(Debug)]
pub struct WallPicture {
    pub image: PartImageId,
    pub fit: String,
    pub focus: (f64, f64),
}

/// A wall picture's bytes: on its row under the inline line, or in the store by hash.
#[derive(Debug)]
pub struct WallBytes {
    pub inline_webp: Option<Vec<u8>>,
    pub hash: Option<BlobHash>,
}

pub struct PgWall(pub PgPool);

impl PgWall {
    /// Up to `limit` pictures, in a new random order each time.
    // ponytail: ORDER BY random() reads every shared part; sample with TABLESAMPLE if a library outgrows it.
    pub async fn pictures(&self, limit: u32) -> Result<Vec<WallPicture>, DbError> {
        let rows: Vec<(Uuid, String, f64, f64)> = sqlx::query_as(concat!(
            "SELECT i.id, i.fit, i.focus_x, i.focus_y",
            on_the_wall!(),
            " ORDER BY random() LIMIT $1"
        ))
        .bind(i64::from(limit))
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(image, fit, focus_x, focus_y)| WallPicture {
                image: PartImageId::from_uuid(image),
                fit,
                focus: (focus_x, focus_y),
            })
            .collect())
    }

    /// The bytes of `image`, only while the wall shows it. Knowing the id is not enough.
    pub async fn picture(&self, image: PartImageId) -> Result<Option<WallBytes>, DbError> {
        let row: Option<(Option<Vec<u8>>, Option<String>)> = sqlx::query_as(concat!(
            "SELECT i.image_webp, i.blake3",
            on_the_wall!(),
            " AND i.id = $1"
        ))
        .bind(image.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        row.map(|(inline_webp, hex)| {
            let hash = hex
                .map(|hex| {
                    BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                        column: "part_image.blake3",
                        value: hex,
                    })
                })
                .transpose()?;
            Ok(WallBytes { inline_webp, hash })
        })
        .transpose()
    }

    /// Whether a part may show on the wall, as its owner set it.
    pub async fn on_wall(&self, part: PartId) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar("SELECT on_wall FROM part WHERE id = $1")
            .bind(part.as_uuid())
            .fetch_optional(&self.0)
            .await?
            .unwrap_or(true))
    }

    pub async fn set_on_wall(&self, part: PartId, on: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE part SET on_wall = $2 WHERE id = $1")
            .bind(part.as_uuid())
            .bind(on)
            .execute(&self.0)
            .await?;
        Ok(())
    }
}
