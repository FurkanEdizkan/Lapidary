//! What a library holds (A2): the files and folders a person added from the models folder, each
//! with the page it came from and the pictures they chose for it. See migration `0064`.

use crate::DbError;
use lapidary_core::LibraryId;
use sqlx::PgPool;
use uuid::Uuid;

/// One added file or folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    pub id: Uuid,
    pub library: LibraryId,
    /// Relative to the models folder; `''` is the whole of it.
    pub path: String,
    /// `file` or `folder`.
    pub kind: String,
    pub page_url: Option<String>,
    pub fetch_details: bool,
}

type Tuple = (Uuid, Uuid, String, String, Option<String>, bool);

fn row((id, library, path, kind, page_url, fetch_details): Tuple) -> SourceRow {
    SourceRow {
        id,
        library: LibraryId::from_uuid(library),
        path,
        kind,
        page_url,
        fetch_details,
    }
}

/// The columns every read here takes, in [`Tuple`]'s order. A macro so the queries stay static
/// strings, as `grid_columns!` does in `repo.rs`.
macro_rules! columns {
    () => {
        "id, library_id, path, kind, page_url, fetch_details"
    };
}

/// Is `source_path` this source, or under it?
pub fn covers(source: &str, source_path: &str) -> bool {
    source.is_empty()
        || source_path == source
        || source_path
            .strip_prefix(source)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub struct PgSources(pub PgPool);

impl PgSources {
    /// A library's sources, by path.
    pub async fn list(&self, library: LibraryId) -> Result<Vec<SourceRow>, DbError> {
        let rows: Vec<Tuple> = sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM library_source WHERE library_id = $1 ORDER BY path"
        ))
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows.into_iter().map(row).collect())
    }

    /// One source of one library, or `None`.
    pub async fn get(&self, library: LibraryId, id: Uuid) -> Result<Option<SourceRow>, DbError> {
        let found: Option<Tuple> = sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM library_source WHERE library_id = $1 AND id = $2"
        ))
        .bind(library.as_uuid())
        .bind(id)
        .fetch_optional(&self.0)
        .await?;
        Ok(found.map(row))
    }

    /// Add a file or folder. Adding one the library already has keeps it, with the new page and
    /// choice of fetching, so adding again is how a person gives a folder its link afterwards.
    pub async fn add(
        &self,
        library: LibraryId,
        path: &str,
        kind: &str,
        page_url: Option<&str>,
        fetch_details: bool,
    ) -> Result<SourceRow, DbError> {
        let added: Tuple = sqlx::query_as(concat!(
            "INSERT INTO library_source (id, library_id, path, kind, page_url, fetch_details) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (library_id, path) DO UPDATE SET kind = excluded.kind, \
               page_url = coalesce(excluded.page_url, library_source.page_url), \
               fetch_details = excluded.fetch_details \
             RETURNING ",
            columns!()
        ))
        .bind(Uuid::now_v7())
        .bind(library.as_uuid())
        .bind(path)
        .bind(kind)
        .bind(page_url)
        .bind(fetch_details)
        .fetch_one(&self.0)
        .await?;
        Ok(row(added))
    }

    /// A picture for every part found under the source, after the ones it has.
    pub async fn add_image(&self, source: Uuid, webp: &[u8]) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO library_source_image (source_id, position, image_webp) \
             SELECT $1, coalesce(max(position), -1) + 1, $2 FROM library_source_image WHERE source_id = $1",
        )
        .bind(source)
        .bind(webp)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// The source's pictures, in the order they were added.
    pub async fn images(&self, source: Uuid) -> Result<Vec<Vec<u8>>, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT image_webp FROM library_source_image WHERE source_id = $1 ORDER BY position",
        )
        .bind(source)
        .fetch_all(&self.0)
        .await?)
    }

    /// The most specific source a part's path is under, or `None` for a part from an upload or an
    /// import, which was added by being written rather than chosen.
    pub async fn covering(
        &self,
        library: LibraryId,
        source_path: &str,
    ) -> Result<Option<SourceRow>, DbError> {
        Ok(self
            .list(library)
            .await?
            .into_iter()
            .filter(|source| covers(&source.path, source_path))
            .max_by_key(|source| source.path.len()))
    }

    /// Take a file or folder out of the library: the source, and every part under it that no other
    /// source of the library still covers. **Rows only** — the files stay where they are, because the
    /// folder is the person's (A2). `None` for no such source; otherwise how many parts went.
    pub async fn remove(&self, library: LibraryId, id: Uuid) -> Result<Option<u64>, DbError> {
        let mut tx = self.0.begin().await?;
        let path: Option<String> = sqlx::query_scalar(
            "DELETE FROM library_source WHERE library_id = $1 AND id = $2 RETURNING path",
        )
        .bind(library.as_uuid())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(path) = path else {
            return Ok(None);
        };
        let others: Vec<String> =
            sqlx::query_scalar("SELECT path FROM library_source WHERE library_id = $1")
                .bind(library.as_uuid())
                .fetch_all(&mut *tx)
                .await?;
        let candidates: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT id, source_path FROM part WHERE library_id = $1 FOR UPDATE")
                .bind(library.as_uuid())
                .fetch_all(&mut *tx)
                .await?;
        let parts: Vec<Uuid> = candidates
            .into_iter()
            .filter(|(_, part_path)| {
                covers(&path, part_path) && !others.iter().any(|other| covers(other, part_path))
            })
            .map(|(id, _)| id)
            .collect();
        crate::repo::delete_parts(&mut tx, &parts).await?;
        tx.commit().await?;
        Ok(Some(parts.len() as u64))
    }

    /// What ties the models folder to where it is now (A6), for choosing another one. `own` are
    /// Lapidary's own top-level folders in it (`examples`, `uploads`, `imports`).
    pub async fn folder_use(&self, own: &[&str]) -> Result<FolderUse, DbError> {
        let own: Vec<String> = own.iter().map(|name| (*name).to_owned()).collect();
        let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM library_source")
            .fetch_one(&self.0)
            .await?;
        let in_place: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM file WHERE root IN ('library', 'inbox') \
             AND NOT split_part(storage_path, '/', 1) = ANY($1)",
        )
        .bind(&own)
        .fetch_one(&self.0)
        .await?;
        let own_files: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT storage_path FROM file WHERE root IN ('library', 'inbox') AND on_disk \
             AND split_part(storage_path, '/', 1) = ANY($1) ORDER BY storage_path",
        )
        .bind(&own)
        .fetch_all(&self.0)
        .await?;
        Ok(FolderUse {
            sources,
            in_place,
            own_files,
        })
    }
}

/// See [`PgSources::folder_use`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderUse {
    /// Files and folders people added from the models folder.
    pub sources: i64,
    /// Model files read where they are in it, outside Lapidary's own folders.
    pub in_place: i64,
    /// Files Lapidary wrote under its own folders, relative to the models folder: what a new
    /// folder gets a copy of.
    pub own_files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::covers;

    #[test]
    fn a_source_covers_itself_and_what_is_under_it_and_nothing_beside_it() {
        assert!(covers("prints", "prints/benchy.stl"));
        assert!(covers("prints/benchy.stl", "prints/benchy.stl"));
        assert!(covers("", "anything/at/all.stl"));
        assert!(!covers("prints", "prints-old/benchy.stl"));
        assert!(!covers("prints/benchy.stl", "prints/benchy.stl.bak"));
    }
}
