//! Reading a model from the site it was published on: Printables, Thingiverse, MyMiniFactory.
//!
//! A person pastes a model's page URL; [`identify`] says which site and which model, [`Sites::model`]
//! reads its name, creator, licence, tags, pictures and files, [`Sites::download`] fetches one file,
//! and [`Sites::check`] tells the settings page whether a key works. Nothing here writes anything:
//! the worker files what comes back through ordinary ingest (F3, 2026-10-16).
//!
//! Every request goes through [`fetch::Net`], the client that refuses private addresses, follows
//! redirects by hand and drops a site's key on a redirect to another host. A site's Cloudflare
//! challenge is reported as itself, never as a bad key: Thingiverse and MyMiniFactory both answered
//! every request from the development machine with one, which no key or retry from a server fixes.
//!
//! **MakerWorld is refused by name.** Its pages sit behind a Cloudflare challenge and its files need a
//! signed-in browser; reaching them means shipping a challenge solver, which this product will not.

pub mod fetch;
mod myminifactory;
mod printables;
mod thingiverse;

use fetch::{Answer, Ask, FetchError, Net};
use std::time::Duration;

/// A site a model can be imported from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Printables,
    Thingiverse,
    MyMiniFactory,
}

impl Site {
    pub const ALL: [Site; 3] = [Site::Printables, Site::Thingiverse, Site::MyMiniFactory];

    /// The word in URLs, settings rows and source paths.
    pub fn slug(self) -> &'static str {
        match self {
            Site::Printables => "printables",
            Site::Thingiverse => "thingiverse",
            Site::MyMiniFactory => "myminifactory",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Site> {
        Site::ALL.into_iter().find(|site| site.slug() == slug)
    }

    /// How a person would write it.
    pub fn name(self) -> &'static str {
        match self {
            Site::Printables => "Printables",
            Site::Thingiverse => "Thingiverse",
            Site::MyMiniFactory => "MyMiniFactory",
        }
    }

    /// Printables serves free models to anybody; the other two want a key for every request.
    pub fn needs_key(self) -> bool {
        !matches!(self, Site::Printables)
    }
}

/// Why a model could not be read or fetched. Every one is a sentence a person can act on.
#[derive(Debug, thiserror::Error)]
pub enum SiteError {
    #[error(
        "Lapidary imports from Printables, Thingiverse and MyMiniFactory, and {host} is none of them. Download the files from that site and upload them instead."
    )]
    Unsupported { host: String },

    #[error(
        "That does not look like a model's page. Paste the address of the model itself — a printables.com/model/…, thingiverse.com/thing:… or myminifactory.com/object/… page."
    )]
    NotAModelPage,

    #[error(
        "MakerWorld keeps its models behind a browser check that a server cannot pass, so Lapidary does not import from it. Download the files from MakerWorld and upload them instead."
    )]
    MakerWorld,

    #[error(
        "Importing from {} needs your {} key. Add it on the Settings page, check it there, and import again.",
        .site.name(), .site.name()
    )]
    NoKey { site: Site },

    #[error(
        "{} refused the key. Check that it was copied whole and has not expired or been revoked, then save it again on the Settings page.",
        .site.name()
    )]
    BadKey { site: Site },

    #[error("{} has no model {id}. Check the address; the model may have been removed.", .site.name())]
    NotFound { site: Site, id: String },

    #[error(
        "{} would not hand over this file. It may be paid, members-only or not yet bought with the account whose key is in Settings; get it on the site, then upload it.",
        .site.name()
    )]
    NotFree { site: Site },

    #[error(
        "{} answered with a browser check (Cloudflare) instead of the model. That is the site refusing servers, not a problem with your key, and retrying from here will not change it. Download the files from the site and upload them instead.",
        .site.name()
    )]
    Cloudflare { site: Site },

    #[error("{} answered in a shape Lapidary does not recognise ({detail}). The site may have changed its API; download the files and upload them instead.", .site.name())]
    Unexpected { site: Site, detail: String },

    #[error(transparent)]
    Fetch(#[from] FetchError),
}

/// A model as its site describes it.
#[derive(Debug, Clone)]
pub struct Model {
    pub site: Site,
    pub id: String,
    /// The model's page: what the part's source link points back to.
    pub url: String,
    pub name: String,
    pub creator: Option<String>,
    pub license: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    /// The site's own categories for it (MyMiniFactory's); they join the part's tags, never its library category.
    pub categories: Vec<String>,
    /// Picture URLs, the site's chosen one first.
    pub images: Vec<String>,
    pub files: Vec<RemoteFile>,
}

/// One of a model's files, and how to ask for it.
#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub name: String,
    pub size: Option<u64>,
    get: FileRef,
}

#[derive(Debug, Clone)]
enum FileRef {
    /// Printables hands out a signed link per request.
    Printables { id: String },
    /// A URL, and whether the site's key goes with it.
    Url { url: String, keyed: bool },
}

/// One model's file: 512 MB. A real kit's largest file in the owner's corpus is 179 MB.
// ponytail: whole files in memory, as ingest reads them; stream to a staging file if a site's
// models outgrow this.
pub const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const JSON_BYTES: usize = 8 * 1024 * 1024;
const JSON_BUDGET: Duration = Duration::from_secs(30);
const FILE_BUDGET: Duration = Duration::from_secs(10 * 60);

/// Which site, and which model on it, a pasted URL names.
pub fn identify(url: &str) -> Result<(Site, String), SiteError> {
    let parsed = url::Url::parse(url.trim()).map_err(|_| SiteError::NotAModelPage)?;
    let host = parsed
        .host_str()
        .ok_or(SiteError::NotAModelPage)?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let path = parsed.path();
    let found = match host.as_str() {
        "printables.com" => printables::model_id(path).map(|id| (Site::Printables, id)),
        "thingiverse.com" => thingiverse::model_id(path).map(|id| (Site::Thingiverse, id)),
        "myminifactory.com" => myminifactory::model_id(path).map(|id| (Site::MyMiniFactory, id)),
        "makerworld.com" => return Err(SiteError::MakerWorld),
        other => {
            return Err(SiteError::Unsupported {
                host: other.to_owned(),
            });
        }
    };
    found.ok_or(SiteError::NotAModelPage)
}

/// Where each site's API is. [`Sites::public`] is the real one; tests point every site at a
/// fixture server.
#[derive(Debug, Clone)]
pub struct Sites {
    net: Net,
    printables_api: String,
    printables_media: String,
    thingiverse_api: String,
    myminifactory_api: String,
}

impl Sites {
    pub fn public() -> Sites {
        Sites {
            net: Net::public(),
            printables_api: "https://api.printables.com/graphql/".to_owned(),
            printables_media: "https://media.printables.com/".to_owned(),
            thingiverse_api: "https://api.thingiverse.com".to_owned(),
            myminifactory_api: "https://www.myminifactory.com/api/v2".to_owned(),
        }
    }

    /// Every site served from `base` (`http://127.0.0.1:port`), under `/printables/graphql/`,
    /// `/printables-media/`, `/thingiverse` and `/mmf/api/v2`.
    #[cfg(any(test, feature = "testing"))]
    pub fn local(base: &str) -> Sites {
        Sites {
            net: Net::allowing_loopback(),
            printables_api: format!("{base}/printables/graphql/"),
            printables_media: format!("{base}/printables-media/"),
            thingiverse_api: format!("{base}/thingiverse"),
            myminifactory_api: format!("{base}/mmf/api/v2"),
        }
    }

    /// The model `id` on `site`, read with `key` where the site needs one.
    pub async fn model(&self, site: Site, id: &str, key: Option<&str>) -> Result<Model, SiteError> {
        let key = needed(site, key)?;
        match site {
            Site::Printables => printables::model(self, id).await,
            Site::Thingiverse => thingiverse::model(self, id, key).await,
            Site::MyMiniFactory => myminifactory::model(self, id, key).await,
        }
    }

    /// One of `model`'s files.
    pub async fn download(
        &self,
        model: &Model,
        file: &RemoteFile,
        key: Option<&str>,
    ) -> Result<Vec<u8>, SiteError> {
        let (url, keyed) = match &file.get {
            FileRef::Printables { id } => {
                (printables::download_link(self, &model.id, id).await?, false)
            }
            // MyMiniFactory lists the file and withholds its link from an account that cannot have it.
            FileRef::Url { url, .. } if url.is_empty() => {
                return Err(SiteError::NotFree { site: model.site });
            }
            FileRef::Url { url, keyed } => (url.clone(), *keyed),
        };
        let headers = match (keyed, key) {
            (true, Some(key)) => vec![bearer(key)],
            _ => Vec::new(),
        };
        let answer = self
            .net
            .fetch(
                &url,
                Ask {
                    headers,
                    cap: MAX_FILE_BYTES,
                    budget: FILE_BUDGET,
                    json: None,
                },
            )
            .await?;
        refuse(model.site, &answer)?;
        match answer.status {
            200..=299 => Ok(answer.bytes),
            401 | 403 => Err(SiteError::NotFree { site: model.site }),
            404 => Err(SiteError::NotFound {
                site: model.site,
                id: model.id.clone(),
            }),
            status => Err(SiteError::Fetch(FetchError::NotOk { status })),
        }
    }

    /// A picture the model's page shows, through the same checks a pasted picture link takes.
    pub async fn image(&self, url: &str) -> Result<Vec<u8>, SiteError> {
        Ok(self.net.image(url).await?)
    }

    /// Whether `key` works on `site`: the account it belongs to, or why not. Printables needs no
    /// key, so its check is that the site answers at all.
    pub async fn check(&self, site: Site, key: Option<&str>) -> Result<String, SiteError> {
        let key = needed(site, key)?;
        match site {
            Site::Printables => printables::check(self).await,
            Site::Thingiverse => thingiverse::check(self, key).await,
            Site::MyMiniFactory => myminifactory::check(self, key).await,
        }
    }

    async fn get_json(
        &self,
        site: Site,
        url: &str,
        key: Option<&str>,
    ) -> Result<(u16, serde_json::Value), SiteError> {
        let answer = self
            .net
            .fetch(
                url,
                Ask {
                    headers: key.map(bearer).into_iter().collect(),
                    cap: JSON_BYTES,
                    budget: JSON_BUDGET,
                    json: None,
                },
            )
            .await?;
        json_of(site, answer)
    }

    async fn post_json(
        &self,
        site: Site,
        url: &str,
        body: serde_json::Value,
    ) -> Result<(u16, serde_json::Value), SiteError> {
        let answer = self
            .net
            .fetch(
                url,
                Ask {
                    headers: Vec::new(),
                    cap: JSON_BYTES,
                    budget: JSON_BUDGET,
                    json: Some(body),
                },
            )
            .await?;
        json_of(site, answer)
    }
}

/// The key, when the site needs one and there is one; an error naming the site when there is not.
fn needed(site: Site, key: Option<&str>) -> Result<Option<&str>, SiteError> {
    let key = key.map(str::trim).filter(|key| !key.is_empty());
    if site.needs_key() && key.is_none() {
        return Err(SiteError::NoKey { site });
    }
    Ok(key)
}

fn bearer(key: &str) -> (String, String) {
    ("authorization".to_owned(), format!("Bearer {key}"))
}

/// A Cloudflare challenge, or an HTML page where JSON or a file belonged, is the site turning a
/// server away.
fn refuse(site: Site, answer: &Answer) -> Result<(), SiteError> {
    let html = answer.content_type.starts_with("text/html")
        && answer.bytes.trim_ascii_start().starts_with(b"<");
    if answer.cloudflare || (answer.status == 403 && html) {
        return Err(SiteError::Cloudflare { site });
    }
    Ok(())
}

fn json_of(site: Site, answer: Answer) -> Result<(u16, serde_json::Value), SiteError> {
    refuse(site, &answer)?;
    if answer.status == 401 {
        return Err(SiteError::BadKey { site });
    }
    let value = serde_json::from_slice(&answer.bytes).map_err(|err| SiteError::Unexpected {
        site,
        detail: format!("HTTP {}, not JSON: {err}", answer.status),
    })?;
    Ok((answer.status, value))
}

/// A string field, or `None` when it is absent, null or blank.
fn text(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_url_names_its_site_and_model() {
        for (url, site, id) in [
            (
                "https://www.printables.com/model/482910-motorcycle-throttle-stop",
                Site::Printables,
                "482910",
            ),
            (
                "https://www.printables.com/de/model/482910-x/files",
                Site::Printables,
                "482910",
            ),
            (
                "https://www.thingiverse.com/thing:4049220",
                Site::Thingiverse,
                "4049220",
            ),
            (
                "https://thingiverse.com/thing:4049220/files",
                Site::Thingiverse,
                "4049220",
            ),
            (
                "https://www.myminifactory.com/object/3d-print-michelangelo-s-david-in-florence-italy-2052",
                Site::MyMiniFactory,
                "2052",
            ),
        ] {
            assert_eq!(identify(url).expect(url), (site, id.to_owned()), "{url}");
        }
    }

    #[test]
    fn makerworld_another_site_and_a_non_model_page_are_each_refused_by_name() {
        assert!(matches!(
            identify("https://makerworld.com/en/models/1234567"),
            Err(SiteError::MakerWorld)
        ));
        match identify("https://www.cults3d.com/en/3d-model/x") {
            Err(SiteError::Unsupported { host }) => assert_eq!(host, "cults3d.com"),
            other => panic!("{other:?}"),
        }
        for url in [
            "https://www.printables.com/@someone",
            "not a url",
            "https://www.thingiverse.com/",
        ] {
            assert!(
                matches!(identify(url), Err(SiteError::NotAModelPage)),
                "{url}"
            );
        }
    }

    #[test]
    fn a_site_that_needs_a_key_says_so_before_asking() {
        assert!(matches!(
            needed(Site::Thingiverse, Some("  ")),
            Err(SiteError::NoKey {
                site: Site::Thingiverse
            })
        ));
        assert_eq!(needed(Site::Printables, None).expect("no key needed"), None);
    }
}
