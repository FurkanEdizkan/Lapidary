//! Printables: a public GraphQL API, no account needed for a free model.
//!
//! Checked against the live API on 2026-10-16 (model 482910): the `print` query below, pictures at
//! `media.printables.com/` + `filePath`, and `getDownloadLink` answering an anonymous request for a
//! free model with a signed `files.printables.com` link valid a day. Introspection is off, so the
//! field names are the ones those requests proved.

use crate::{FileRef, Model, RemoteFile, Site, SiteError, Sites, text};
use serde_json::{Value, json};

const SITE: Site = Site::Printables;

/// `/model/482910-some-slug`, optionally behind a language segment (`/de/model/…`).
pub(crate) fn model_id(path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let mut segment = segments.next()?;
    if segment.len() == 2 {
        segment = segments.next()?;
    }
    if segment != "model" {
        return None;
    }
    let digits: String = segments
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

const MODEL_QUERY: &str = "query($id: ID!) { print(id: $id) { id name slug summary description \
    license { name abbreviation } user { publicUsername handle } tags { name } \
    images { filePath } stls { id name fileSize } } }";

pub(crate) async fn model(sites: &Sites, id: &str) -> Result<Model, SiteError> {
    let (_, body) = sites
        .post_json(
            SITE,
            &sites.printables_api,
            json!({ "query": MODEL_QUERY, "variables": { "id": id } }),
        )
        .await?;
    errors(&body)?;
    let print = &body["data"]["print"];
    if print.is_null() {
        return Err(SiteError::NotFound {
            site: SITE,
            id: id.to_owned(),
        });
    }
    let name = text(&print["name"]).ok_or_else(|| unexpected("a model with no name"))?;
    let slug = text(&print["slug"]).unwrap_or_default();
    let files = print["stls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some(RemoteFile {
                name: text(&file["name"])?,
                size: file["fileSize"].as_u64(),
                get: FileRef::Printables {
                    id: text(&file["id"]).or_else(|| file["id"].as_u64().map(|n| n.to_string()))?,
                },
            })
        })
        .collect();
    Ok(Model {
        site: SITE,
        id: id.to_owned(),
        url: format!("https://www.printables.com/model/{id}-{slug}")
            .trim_end_matches('-')
            .to_owned(),
        name,
        creator: text(&print["user"]["publicUsername"]).or_else(|| text(&print["user"]["handle"])),
        license: text(&print["license"]["name"]),
        description: text(&print["summary"]).or_else(|| text(&print["description"])),
        tags: print["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tag| text(&tag["name"]))
            .collect(),
        categories: Vec::new(),
        images: print["images"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|image| text(&image["filePath"]))
            .map(|path| format!("{}{}", sites.printables_media, path.trim_start_matches('/')))
            .collect(),
        files,
    })
}

/// A signed link to one file, asked for when it is about to be fetched: they expire.
pub(crate) async fn download_link(
    sites: &Sites,
    print: &str,
    file: &str,
) -> Result<String, SiteError> {
    let mutation = "mutation($id: ID!, $printId: ID!) { getDownloadLink(id: $id, printId: $printId, \
        fileType: stl, source: model_detail) { ok errors { field messages } output { link } } }";
    let (_, body) = sites
        .post_json(
            SITE,
            &sites.printables_api,
            json!({ "query": mutation, "variables": { "id": file, "printId": print } }),
        )
        .await?;
    errors(&body)?;
    let answer = &body["data"]["getDownloadLink"];
    if answer["ok"].as_bool() != Some(true) {
        // `files_cannot_be_downloaded`: a paid or club model, or a file id that is not this model's.
        return Err(SiteError::NotFree { site: SITE });
    }
    text(&answer["output"]["link"]).ok_or_else(|| unexpected("a download answer with no link"))
}

/// Printables has no account to name: `Ok` means it answered.
pub(crate) async fn check(sites: &Sites) -> Result<String, SiteError> {
    let (_, body) = sites
        .post_json(
            SITE,
            &sites.printables_api,
            json!({ "query": "{ __typename }" }),
        )
        .await?;
    errors(&body)?;
    Ok(String::new())
}

fn errors(body: &Value) -> Result<(), SiteError> {
    match body["errors"].as_array().and_then(|errors| errors.first()) {
        Some(error) => Err(unexpected(&format!(
            "GraphQL error: {}",
            text(&error["message"]).unwrap_or_default()
        ))),
        None => Ok(()),
    }
}

fn unexpected(detail: &str) -> SiteError {
    SiteError::Unexpected {
        site: SITE,
        detail: detail.to_owned(),
    }
}
