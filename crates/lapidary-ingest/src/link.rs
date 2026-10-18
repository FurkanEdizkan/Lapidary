//! `import_link`: a model from the page a person pasted (F3, 2026-10-16).
//!
//! The site is read with the key saved in Settings, each of the model's mesh files is fetched and
//! written once into the inbox at `imports/<site>/<creator>/<model>/<file>` and indexed from there
//! through [`WorkerHandler::index`] — the path a scan and an upload take, hash first — as the part
//! `<site>/<creator>/<model>/<file>`, and then each part gets what the page said about it: a source
//! row naming the page, the site, the model's id and its licence; the creator's tags when the part has
//! none; every picture on the page, to [`MAX_PICTURES`], after the ones it has and once per URL (M5).
//! What a person already gave a part wins.
//!
//! Nothing is written before a file's bytes are in hand: a file that cannot be fetched fails the job
//! before ingest sees it, so there is no staging directory to leak. A re-import of the same model is
//! a re-scan: the same bytes are `skipped`, a changed file becomes the part's next revision, and the
//! source row is not added twice.
//!
//! **Archives are skipped, named in the log.** A creator's pack (`.zip`, `.7z`, `.rar`) is P1's to
//! open; a model with nothing else says so rather than importing nothing in silence.

use crate::handler::{FileAt, WorkerHandler};
use lapidary_core::slug::slugify;
use lapidary_core::{
    BlobHash, FileRoot, LibraryId, Outcome, RevisionOrigin, is_model_file, source_format,
};
use lapidary_db::{NewPartSource, PgParts, PgRevisions, PgSiteKeys};
use lapidary_jobs::HandlerError;
use lapidary_sites::{Site, SiteError, identify};
use lapidary_storage::InboxWriter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A model's page as read once: the site, its description and its pictures (A2). A folder added
/// with a page link describes every part in it from one read, so the read is kept for a while.
pub(crate) struct Page {
    site: Site,
    model: lapidary_sites::Model,
    pictures: Vec<(String, Vec<u8>)>,
}

/// How long a read page is reused, and how many are kept.
///
/// `ponytail:` a process-wide map with a ten-minute life and a cap of 16, evicted on use. Long enough
/// for a scan of one added folder, short enough that a page edited on its site is read again; a
/// cache with its own owner is the upgrade if two workers ever need to share one.
const PAGE_LIFE: Duration = Duration::from_secs(600);
const PAGES_KEPT: usize = 16;
/// Each read page by its URL, with when it was read.
type Pages = Mutex<HashMap<String, (Instant, Arc<Page>)>>;
static PAGES: OnceLock<Pages> = OnceLock::new();

/// The most tags a page gives a part; past this a creator's tag list is a keyword dump.
const MAX_TAGS: usize = 20;
/// A page's description is kept to this many characters; past it is a page pasted whole, not a description.
const MAX_DESCRIPTION: usize = 20_000;
/// A page's pictures past this many are its gallery, not the model's.
const MAX_PICTURES: usize = 12;

impl WorkerHandler {
    /// `describe_page`: a part already in the library described from its page — source row, tags,
    /// pictures — and nothing downloaded (A2). What "Fetch details" asks for, and what a file added
    /// with a page link gets once it is filed.
    pub(crate) async fn describe_one(
        &self,
        library: LibraryId,
        source_path: &str,
        url: &str,
    ) -> Result<Outcome, HandlerError> {
        let page = self.page(library, url).await?;
        self.describe_from_page(library, source_path, page.site, &page.model, &page.pictures)
            .await;
        Ok(Outcome::Described)
    }

    /// The page at `url`, read through the library owner's key, from the cache when that person read
    /// it lately. Kept per person (A3): what a page shows can depend on whose key read it.
    async fn page(&self, library: LibraryId, url: &str) -> Result<Arc<Page>, HandlerError> {
        let pages = PAGES.get_or_init(|| Mutex::new(HashMap::new()));
        let owner = self.owner_of(library).await?;
        let cached = format!("{}:{url}", owner.map(|o| o.to_string()).unwrap_or_default());
        if let Ok(mut kept) = pages.lock() {
            kept.retain(|_, (read, _)| read.elapsed() < PAGE_LIFE);
            if let Some((_, page)) = kept.get(&cached) {
                return Ok(page.clone());
            }
        }
        let (site, id) = identify(url).map_err(permanent)?;
        let key = self.site_key(library, site).await?;
        let model = self
            .sites
            .model(site, &id, key.as_deref())
            .await
            .map_err(classify)?;
        let pictures = self.pictures(site, &model).await;
        let page = Arc::new(Page {
            site,
            model,
            pictures,
        });
        if let Ok(mut kept) = pages.lock() {
            if kept.len() >= PAGES_KEPT
                && let Some(oldest) = kept
                    .iter()
                    .min_by_key(|(_, (read, _))| *read)
                    .map(|(url, _)| url.clone())
            {
                kept.remove(&oldest);
            }
            kept.insert(cached, (Instant::now(), page.clone()));
        }
        Ok(page)
    }

    /// Who the library belongs to (A3): whose keys read its pages. `None` before setup claims it.
    async fn owner_of(
        &self,
        library: LibraryId,
    ) -> Result<Option<lapidary_core::UserId>, HandlerError> {
        PgParts(self.db.clone())
            .library_owner(library)
            .await
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })
    }

    /// The library owner's key for `site` (A3): each person imports with their own.
    async fn site_key(
        &self,
        library: LibraryId,
        site: Site,
    ) -> Result<Option<String>, HandlerError> {
        if !site.needs_key() {
            return Ok(None);
        }
        let Some(owner) = self.owner_of(library).await? else {
            return Ok(None);
        };
        PgSiteKeys(self.db.clone(), owner)
            .secret(site.slug())
            .await
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })
    }

    /// The page's pictures, to [`MAX_PICTURES`], each fetched once. One that fails is left out and
    /// said in the log; the rest still come.
    async fn pictures(&self, site: Site, model: &lapidary_sites::Model) -> Vec<(String, Vec<u8>)> {
        let mut pictures = Vec::new();
        for url in model.images.iter().take(MAX_PICTURES) {
            match self.sites.image(url).await {
                Ok(bytes) => pictures.push((url.clone(), bytes)),
                Err(error) => {
                    tracing::warn!(site = site.slug(), %url, %error, "could not fetch one of the model's pictures; its parts go without it");
                }
            }
        }
        pictures
    }

    pub(crate) async fn import_link(
        &self,
        library: LibraryId,
        url: &str,
    ) -> Result<Outcome, HandlerError> {
        let (site, id) = identify(url).map_err(permanent)?;
        let key = self.site_key(library, site).await?;
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

        // Fetched once, each attached to every part of the set (M5).
        let pictures = self.pictures(site, &model).await;

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
            // Into Lapidary's own part of the inbox, once (M4), then indexed from there like any
            // other file. A re-import of unchanged bytes leaves the file as it was.
            let placed = InboxWriter::open(&self.inbox_dir)
                .put(&format!("imports/{source_path}"), &bytes)
                .map_err(|e| HandlerError::Transient {
                    message: format!("Could not write {source_path} into the inbox: {e}"),
                })?;
            let at = FileAt {
                root: FileRoot::Inbox,
                path: &placed.path,
                mtime_ns: placed.stat.mtime_ns,
            };
            let filed = self
                .index(
                    library,
                    &source_path,
                    bytes,
                    hash,
                    RevisionOrigin::Upload,
                    at,
                )
                .await?;
            outcome = most_telling(outcome, filed);
            self.describe_from_page(library, &source_path, site, &model, &pictures)
                .await;
        }
        Ok(outcome)
    }

    /// The source row and the tags, each only where the part lacks it, and the page's pictures it lacks. Warn-only: the
    /// part is filed, and what the page adds is not a reason to fail it.
    async fn describe_from_page(
        &self,
        library: LibraryId,
        source_path: &str,
        site: Site,
        model: &lapidary_sites::Model,
        pictures: &[(String, Vec<u8>)],
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
        let description: Option<String> = model
            .description
            .as_deref()
            .map(|text| text.chars().take(MAX_DESCRIPTION).collect());
        match parts.part_sources(part).await {
            // Already recorded: what a person may have edited on it stays, and only a missing description is filled.
            Ok(sources)
                if sources
                    .iter()
                    .any(|s| s.url.as_deref() == Some(model.url.as_str())) =>
            {
                if let Some(description) = &description
                    && let Err(error) = parts
                        .fill_source_description(part, &model.url, description)
                        .await
                {
                    tracing::warn!(source_path, %error, "could not record the page's description");
                }
            }
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
                            description: description.as_deref(),
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
        // The page's tags and the site's categories join the part's own tags (A15): none is replaced, none doubled.
        if let Ok(Some(detail)) = parts.detail(part).await {
            let mut tags = detail.tags.clone();
            for tag in model.tags.iter().chain(&model.categories) {
                let tag = tag.trim().to_lowercase();
                if !tag.is_empty()
                    && !tags.contains(&tag)
                    && tags.len() < MAX_TAGS.max(detail.tags.len())
                {
                    tags.push(tag);
                }
            }
            if tags != detail.tags
                && let Err(error) = parts.set_tags(part, &tags).await
            {
                tracing::warn!(source_path, %error, "could not record the imported part's tags");
            }
        }
        for (url, bytes) in pictures {
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
