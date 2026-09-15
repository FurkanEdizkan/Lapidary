//! A density per material, per library: what a part's mass is worked out from (goal 5).
//!
//! Keyed by the material exactly as parts hold it, capitals and all, as the materials facet keeps it.
//! A density is typed by a person, never measured.

use crate::DbError;
use lapidary_core::LibraryId;
use sqlx::PgPool;

/// One material's density in one library.
#[derive(Debug, Clone, PartialEq)]
pub struct DensityRow {
    pub material: String,
    pub density_kg_m3: f64,
}

pub struct PgDensities(pub PgPool);

impl PgDensities {
    /// A library's densities, by material. An id naming no library has none.
    pub async fn list(&self, library: LibraryId) -> Result<Vec<DensityRow>, DbError> {
        let rows: Vec<(String, f64)> = sqlx::query_as(
            "SELECT material, density_kg_m3::float8 FROM material_density \
             WHERE library_id = $1 ORDER BY material",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(material, density_kg_m3)| DensityRow {
                material,
                density_kg_m3,
            })
            .collect())
    }

    /// Set a material's density, replacing the one it had. `false` when no library has that id. The
    /// bounds are the API's to say in words; the table's check refuses anything past them.
    pub async fn set(
        &self,
        library: LibraryId,
        material: &str,
        density_kg_m3: f64,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            "INSERT INTO material_density (library_id, material, density_kg_m3) \
             SELECT id, $2, $3::numeric FROM library WHERE id = $1 \
             ON CONFLICT (library_id, material) DO UPDATE SET density_kg_m3 = EXCLUDED.density_kg_m3",
        )
        .bind(library.as_uuid())
        .bind(material)
        .bind(density_kg_m3)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Remove a material's density. `false` when it had none in that library.
    pub async fn remove(&self, library: LibraryId, material: &str) -> Result<bool, DbError> {
        let removed =
            sqlx::query("DELETE FROM material_density WHERE library_id = $1 AND material = $2")
                .bind(library.as_uuid())
                .bind(material)
                .execute(&self.0)
                .await?;
        Ok(removed.rows_affected() == 1)
    }
}
