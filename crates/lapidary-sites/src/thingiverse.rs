//! Thingiverse: a REST API that wants an app token on every request, even for a public thing.
//!
//! **Not checked live**: every Thingiverse host answered the development machine with a Cloudflare
//! challenge on 2026-10-16. Built from responses other open-source importers recorded (manyfold's test
//! cassettes, MeshDepot's client): `GET /things/{id}`, `/things/{id}/files`, `/things/{id}/images`,
//! and `GET /users/me` to check a token. A file's `direct_url` is a CDN link fetched without the
//! token; `download_url` is the API's own, which takes it and redirects.

use crate::{FileRef, Model, RemoteFile, Site, SiteError, Sites, text};
use serde_json::Value;

const SITE: Site = Site::Thingiverse;

/// `/thing:4049220`, and anything after it.
pub(crate) fn model_id(path: &str) -> Option<String> {
    let rest = path.trim_start_matches('/').strip_prefix("thing:")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

pub(crate) async fn model(sites: &Sites, id: &str, key: Option<&str>) -> Result<Model, SiteError> {
    let api = &sites.thingiverse_api;
    let (status, thing) = sites
        .get_json(SITE, &format!("{api}/things/{id}"), key)
        .await?;
    found(status, id)?;
    let name = text(&thing["name"]).ok_or_else(|| unexpected("a thing with no name"))?;

    let (status, files) = sites
        .get_json(SITE, &format!("{api}/things/{id}/files"), key)
        .await?;
    found(status, id)?;
    let files = files
        .as_array()
        .ok_or_else(|| unexpected("a file list that is not a list"))?
        .iter()
        .filter_map(|file| {
            let get = match text(&file["direct_url"]) {
                Some(url) => FileRef::Url { url, keyed: false },
                None => FileRef::Url {
                    url: text(&file["download_url"])?,
                    keyed: true,
                },
            };
            Some(RemoteFile {
                name: text(&file["name"])?,
                size: file["size"].as_u64(),
                get,
            })
        })
        .collect();

    // The large display picture of each image, which is what both importers this was built from pick;
    // the thing's own default picture when the list has none.
    let (status, images) = sites
        .get_json(SITE, &format!("{api}/things/{id}/images"), key)
        .await?;
    let mut pictures: Vec<String> = if (200..300).contains(&status) {
        images
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|image| {
                image["sizes"].as_array()?.iter().find_map(|size| {
                    (size["type"] == "display" && size["size"] == "large")
                        .then(|| text(&size["url"]))?
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    if pictures.is_empty()
        && let Some(url) = text(&thing["default_image"]["url"])
    {
        pictures.push(url);
    }

    Ok(Model {
        site: SITE,
        id: id.to_owned(),
        url: text(&thing["public_url"])
            .unwrap_or_else(|| format!("https://www.thingiverse.com/thing:{id}")),
        name,
        creator: text(&thing["creator"]["name"]),
        license: text(&thing["license"]),
        description: text(&thing["description"]),
        tags: tags(&thing["tags"]),
        images: pictures,
        files,
    })
}

pub(crate) async fn check(sites: &Sites, key: Option<&str>) -> Result<String, SiteError> {
    let (status, me) = sites
        .get_json(SITE, &format!("{}/users/me", sites.thingiverse_api), key)
        .await?;
    if !(200..300).contains(&status) {
        return Err(SiteError::BadKey { site: SITE });
    }
    Ok(text(&me["name"]).unwrap_or_else(|| "a Thingiverse account".to_owned()))
}

fn found(status: u16, id: &str) -> Result<(), SiteError> {
    match status {
        200..=299 => Ok(()),
        404 => Err(SiteError::NotFound {
            site: SITE,
            id: id.to_owned(),
        }),
        403 => Err(SiteError::BadKey { site: SITE }),
        other => Err(unexpected(&format!("HTTP {other}"))),
    }
}

fn tags(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tag| text(&tag["name"]).or_else(|| text(tag)))
        .collect()
}

fn unexpected(detail: &str) -> SiteError {
    SiteError::Unexpected {
        site: SITE,
        detail: detail.to_owned(),
    }
}
