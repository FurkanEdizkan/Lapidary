//! MyMiniFactory: API v2, read with the person's **API key** or their **OAuth access token**.
//!
//! The two are sent differently, and each is refused the other way. Probed live with an owner's API key on
//! 2026-10-18 (A14): `?key=` answers an object, `Authorization: Bearer` with the same key is 401, and `/user`
//! answers only a token. So the secret goes as `?key=` first and, on a 401, again as `Bearer`. An API key gets
//! everything but a file's `download_url`, which the official OpenAPI file (`MyMiniFactory/api-documentation`)
//! says is "available ONLY with Oauth connected User. Not with API key" — it is `null` then.
//!
//! curl is met with a Cloudflare challenge on these hosts (its TLS fingerprint); reqwest and Python are not.

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
    let (status, object) = get(sites, &format!("objects/{id}"), key).await?;
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
                // A number in the spec, a string ("56714948") in the live answer.
                size: file["size"]
                    .as_u64()
                    .or_else(|| file["size"].as_str()?.parse().ok()),
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
        categories: object["categories"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|category| text(&category["name"]))
            .collect(),
        images: images.into_iter().map(|(_, url)| url).collect(),
        files,
    })
}

pub(crate) async fn check(sites: &Sites, key: Option<&str>) -> Result<String, SiteError> {
    // `/categories` refuses a bad or missing key, and needs no account behind it.
    match keyed(sites, "categories?per_page=1", key).await {
        Ok((200..=299, _)) => {
            return Ok("reading models and their pictures (an API key; downloading files needs an account's access token)".to_owned());
        }
        Ok(_) | Err(SiteError::BadKey { .. }) => {}
        Err(other) => return Err(other),
    }
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

/// `path` under the API, with the secret as an API key and then, if that is refused, as a token.
async fn get(
    sites: &Sites,
    path: &str,
    key: Option<&str>,
) -> Result<(u16, serde_json::Value), SiteError> {
    match keyed(sites, path, key).await {
        Err(SiteError::BadKey { .. }) if key.is_some() => {
            let url = format!("{}/{path}", sites.myminifactory_api);
            sites.get_json(SITE, &url, key).await
        }
        answer => answer,
    }
}

/// `path` with `key=` added to its query; reqwest's errors never print the URL (`fetch.rs`).
async fn keyed(
    sites: &Sites,
    path: &str,
    key: Option<&str>,
) -> Result<(u16, serde_json::Value), SiteError> {
    let mut url = format!("{}/{path}", sites.myminifactory_api);
    if let Some(key) = key {
        let key: String = url::form_urlencoded::byte_serialize(key.as_bytes()).collect();
        url.push(if path.contains('?') { '&' } else { '?' });
        url.push_str("key=");
        url.push_str(&key);
    }
    sites.get_json(SITE, &url, None).await
}

fn unexpected(detail: &str) -> SiteError {
    SiteError::Unexpected {
        site: SITE,
        detail: detail.to_owned(),
    }
}
