//! MyMiniFactory: API v2, read with the person's **OAuth access token**.
//!
//! The official OpenAPI file (`MyMiniFactory/api-documentation`, `myminifactory-api.yaml`) says a
//! file's `download_url` is "available ONLY with Oauth connected User. Not with API key" — and a
//! response recorded with a key has it `null`. So the key Settings asks for is the access token, sent
//! as `Authorization: Bearer`, and checked with `GET /user`, which only a token can answer.
//!
//! **Not checked live**: MyMiniFactory's hosts answered the development machine with a Cloudflare
//! challenge on 2026-10-16. Built from that spec and a response manyfold recorded (object 2052).

use crate::{FileRef, Model, RemoteFile, Site, SiteError, Sites, text};

const SITE: Site = Site::MyMiniFactory;

/// `/object/3d-print-some-name-12345`: the number at the end of the slug.
pub(crate) fn model_id(path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    if segments.next()? != "object" {
        return None;
    }
    let slug = segments.next()?;
    let digits: String = slug
        .rsplit('-')
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

pub(crate) async fn model(sites: &Sites, id: &str, key: Option<&str>) -> Result<Model, SiteError> {
    let (status, object) = sites
        .get_json(
            SITE,
            &format!("{}/objects/{id}", sites.myminifactory_api),
            key,
        )
        .await?;
    match status {
        200..=299 => {}
        404 => {
            return Err(SiteError::NotFound {
                site: SITE,
                id: id.to_owned(),
            });
        }
        403 => return Err(SiteError::NotFree { site: SITE }),
        other => return Err(unexpected(&format!("HTTP {other}"))),
    }
    let name = text(&object["name"]).ok_or_else(|| unexpected("an object with no name"))?;
    let files = object["files"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some(RemoteFile {
                name: text(&file["filename"])?,
                size: file["size"].as_u64(),
                // `null` without a token, or for an object this account has not bought: kept as an
                // empty URL so the import can say which, rather than the file vanishing.
                get: FileRef::Url {
                    url: text(&file["download_url"]).unwrap_or_default(),
                    keyed: true,
                },
            })
        })
        .collect();
    // The primary picture first, each at the largest size the API lists.
    let mut images: Vec<(bool, String)> = object["images"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|image| {
            let url = ["large", "standard", "original", "thumbnail"]
                .into_iter()
                .find_map(|size| text(&image[size]["url"]))?;
            Some((image["is_primary"].as_bool() == Some(true), url))
        })
        .collect();
    images.sort_by_key(|(primary, _)| !*primary);
    Ok(Model {
        site: SITE,
        id: id.to_owned(),
        url: text(&object["url"])
            .unwrap_or_else(|| format!("https://www.myminifactory.com/object/{id}")),
        name,
        creator: text(&object["designer"]["name"])
            .or_else(|| text(&object["designer"]["username"])),
        license: text(&object["license"]),
        description: text(&object["description"]),
        tags: object["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(text)
            .collect(),
        images: images.into_iter().map(|(_, url)| url).collect(),
        files,
    })
}

pub(crate) async fn check(sites: &Sites, key: Option<&str>) -> Result<String, SiteError> {
    let (status, me) = sites
        .get_json(SITE, &format!("{}/user", sites.myminifactory_api), key)
        .await?;
    if !(200..300).contains(&status) {
        return Err(SiteError::BadKey { site: SITE });
    }
    Ok(text(&me["username"])
        .or_else(|| text(&me["name"]))
        .unwrap_or_else(|| "a MyMiniFactory account".to_owned()))
}

fn unexpected(detail: &str) -> SiteError {
    SiteError::Unexpected {
        site: SITE,
        detail: detail.to_owned(),
    }
}
