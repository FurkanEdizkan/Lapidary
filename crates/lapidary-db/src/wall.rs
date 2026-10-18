//! The sign-in wall (A28): the pictures a signed-out page may show. Only the first picture of a part shared with
//! everyone, not hidden from the wall by its owner, while the admin has not switched the wall off — and nothing else
//! about the part. Both reads carry the whole condition, so the picture route can never serve what the list would not.

use crate::DbError;
use lapidary_core::{BlobHash, PartId};
use sqlx::PgPool;

/// The condition, once: `p` is the part, `i` its first picture.
macro_rules! on_the_wall {
    () => {
        " FROM part p \
         JOIN LATERAL (SELECT image_webp, blake3, fit, focus_x, focus_y FROM part_image \
                       WHERE part_id = p.id ORDER BY position, id LIMIT 1) i ON true \
         WHERE p.shared_with_everyone AND p.on_wall \
           AND coalesce((SELECT value FROM instance_setting WHERE key = 'sign_in_wall'), 'true') = 'true'"
    };
}

/// One picture on the wall: its bytes or its hash, and how it sits in its frame.
#[derive(Debug)]
pub struct WallPicture {
    pub inline_webp: Option<Vec<u8>>,
    pub hash: Option<BlobHash>,
    pub fit: String,
    pub focus: (f64, f64),
}

pub struct PgWall(pub PgPool);

impl PgWall {
    /// Up to `limit` pictures, in a new random order each time.
    // ponytail: ORDER BY random() reads every shared part; sample with TABLESAMPLE if a library outgrows it.
    pub async fn pictures(&self, limit: u32) -> Result<Vec<WallPicture>, DbError> {
        let rows: Vec<(Option<Vec<u8>>, Option<String>, String, f64, f64)> =
            sqlx::query_as(concat!(
                "SELECT i.image_webp, i.blake3, i.fit, i.focus_x, i.focus_y",
                on_the_wall!(),
                " ORDER BY random() LIMIT $1"
            ))
            .bind(i64::from(limit))
            .fetch_all(&self.0)
            .await?;
        rows.into_iter()
            .map(|(inline_webp, hex, fit, focus_x, focus_y)| {
                let hash = hex
                    .map(|hex| {
                        BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                            column: "part_image.blake3",
                            value: hex,
                        })
                    })
                    .transpose()?;
                Ok(WallPicture {
                    inline_webp,
                    hash,
                    fit,
                    focus: (focus_x, focus_y),
                })
            })
            .collect()
    }

    /// Whether these bytes are a picture the wall shows now. Knowing the hash is not enough.
    pub async fn shows(&self, hash: &BlobHash) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(concat!(
            "SELECT EXISTS (SELECT 1",
            on_the_wall!(),
            " AND i.blake3 = $1)"
        ))
        .bind(hash.to_hex())
        .fetch_one(&self.0)
        .await?)
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
