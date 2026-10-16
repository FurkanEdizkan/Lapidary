//! `import_link`: a model from the page a person pasted (F3, 2026-10-16).
//!
//! The site is read with the key saved in Settings, each of the model's mesh files is fetched and
//! filed through [`WorkerHandler::index`] — the path a scan and an upload take, hash first — under
//! `<site>/<creator>/<model>/<file>`, and then each part gets what the page said about it: a source
//! row naming the page, the site, the model's id and its licence; the creator's tags when the part has
//! none; the page's first picture when the part has none. What a person already gave a part wins.
//!
//! Nothing is written before a file's bytes are in hand: a file that cannot be fetched fails the job
//! before ingest sees it, so there is no staging directory to leak. A re-import of the same model is
//! a re-scan: the same bytes are `skipped`, a changed file becomes the part's next revision, and the
//! source row is not added twice.
//!
//! **Archives are skipped, named in the log.** A creator's pack (`.zip`, `.7z`, `.rar`) is P1's to
//! open; a model with nothing else says so rather than importing nothing in silence.

use crate::handler::WorkerHandler;
use lapidary_core::slug::slugify;
use lapidary_core::{BlobHash, LibraryId, Outcome, RevisionOrigin, is_model_file, source_format};
use lapidary_db::{NewPartSource, PgParts, PgRevisions, PgSiteKeys};
use lapidary_jobs::HandlerError;
use lapidary_sites::{Site, SiteError, identify};

/// The most tags a page gives a part; past this a creator's tag list is a keyword dump.
const MAX_TAGS: usize = 20;

impl WorkerHandler {
    pub(crate) async fn import_link(
        &self,
        library: LibraryId,
        url: &str,
    ) -> Result<Outcome, HandlerError> {
        let (site, id) = identify(url).map_err(permanent)?;
        let key = if site.needs_key() {
            PgSiteKeys(self.db.clone())
                .secret(site.slug())
                .await
                .map_err(|e| HandlerError::Transient {
                    message: e.to_string(),
                })?
        } else {
            None
        };
        let model = self
            .sites
            .model(site, &id, key.as_deref())
            .await
            .map_err(classify)?;

        let (meshes, skipped): (Vec<_>, Vec<_>) = model
            .files
            .iter()
            .partition(|file| is_model_file(&file.name) && !is_cad(&file.name));
        for file in &skipped {
            tracing::info!(site = site.slug(), model = %model.id, file = %file.name, "not a mesh Lapidary reads; skipped");
        }
        if meshes.is_empty() {
            return Err(HandlerError::Permanent {
                message: format!(
                    "{} lists no STL, OBJ or 3MF file for “{}” — only {}. Download what it has and upload the meshes; a creator's archive is opened once packs are supported.",
                    site.name(),
                    model.name,
                    if skipped.is_empty() {
                        "nothing".to_owned()
                    } else {
                        skipped
                            .iter()
                            .map(|f| f.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ),
            });
        }

        // Fetched once, attached to each part that has no picture.
        let picture = match model.images.first() {
            Some(url) => match self.sites.image(url).await {
                Ok(bytes) => Some((url.clone(), bytes)),
                Err(error) => {
                    tracing::warn!(site = site.slug(), %error, "could not fetch the model's picture; its parts have none from it");
                    None
                }
            },
            None => None,
        };

        let folder = format!(
            "{}/{}/{}",
            site.slug(),
            slugify(model.creator.as_deref().unwrap_or("unknown")),
            slugify(&model.name),
        );
        let mut outcome = Outcome::Skipped;
        for file in meshes {
            let bytes = self
                .sites
                .download(&model, file, key.as_deref())
                .await
                .map_err(classify)?;
            let source_path = format!("{folder}/{}", file.name.replace(['/', '\\'], "_"));
            let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
            let filed = self
                .index(library, &source_path, bytes, hash, RevisionOrigin::Upload)
                .await?;
            outcome = most_telling(outcome, filed);
            self.describe_from_page(library, &source_path, site, &model, picture.as_ref())
                .await;
        }
        Ok(outcome)
    }

    /// The source row, the tags and the picture, each only where the part lacks it. Warn-only: the
    /// part is filed, and what the page adds is not a reason to fail it.
    async fn describe_from_page(
        &self,
        library: LibraryId,
        source_path: &str,
        site: Site,
        model: &lapidary_sites::Model,
        picture: Option<&(String, Vec<u8>)>,
    ) {
        let part = match PgRevisions(self.db.clone())
            .current(library, source_path)
            .await
        {
            Ok(Some(current)) => current.part,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(source_path, %error, "could not find the imported part again; its source and tags are not recorded");
                return;
            }
        };
        let parts = PgParts(self.db.clone());
        match parts.part_sources(part).await {
            Ok(sources)
                if sources
                    .iter()
                    .any(|s| s.url.as_deref() == Some(model.url.as_str())) => {}
            Ok(_) => {
                let written = parts
                    .add_part_source(
                        part,
                        NewPartSource {
                            url: Some(&model.url),
                            vendor: Some(site.name()),
                            external_id: Some(&model.id),
                            title: Some(&model.name),
                            license: model.license.as_deref(),
                            ..NewPartSource::default()
                        },
                    )
                    .await;
                if let Err(error) = written {
                    tracing::warn!(source_path, %error, "could not record where the imported part came from");
                }
            }
            Err(error) => {
                tracing::warn!(source_path, %error, "could not read the imported part's sources")
            }
        }
        if !model.tags.is_empty()
            && let Ok(Some(detail)) = parts.detail(part).await
            && detail.tags.is_empty()
        {
            let tags: Vec<String> = model
                .tags
                .iter()
                .map(|tag| tag.trim().to_lowercase())
                .filter(|tag| !tag.is_empty())
                .take(MAX_TAGS)
                .collect();
            if let Err(error) = parts.set_tags(part, &tags).await {
                tracing::warn!(source_path, %error, "could not record the imported part's tags");
            }
        }
        if let Some((url, bytes)) = picture {
            self.adopt_picture(part, bytes, "url_supplied", Some(url), source_path)
                .await;
        }
    }
}

/// STEP and IGES pass `is_model_file` so a scan can name them; an import has nothing to say about a
/// file it was never going to read, so it skips them with the other non-meshes.
fn is_cad(name: &str) -> bool {
    lapidary_core::CAD_FORMATS.contains(&source_format(name).as_str())
}

/// One answer for the job: a new part says more than a revision, which says more than a skip.
fn most_telling(so_far: Outcome, next: Outcome) -> Outcome {
    let rank = |o: Outcome| match o {
        Outcome::Ingested => 3,
        Outcome::Revised => 2,
        _ => 1,
    };
    if rank(next) > rank(so_far) {
        next
    } else {
        so_far
    }
}

fn permanent(error: SiteError) -> HandlerError {
    HandlerError::Permanent {
        message: error.to_string(),
    }
}

/// A site that did not answer, or timed out, may answer on a retry; a model that is gone, paid or
/// behind a browser check will not, and neither will a missing or refused key until a person acts.
fn classify(error: SiteError) -> HandlerError {
    use lapidary_sites::fetch::FetchError as F;
    match error {
        SiteError::Fetch(F::TimedOut | F::Unreachable { .. } | F::Unresolvable)
        | SiteError::Fetch(F::NotOk { status: 500..=599 }) => HandlerError::Transient {
            message: error.to_string(),
        },
        other => permanent(other),
    }
}
