use std::{collections::HashMap, error::Error, num::NonZeroUsize};
use regex::Regex;
use reqwest::{header::{HeaderMap, HeaderName, HeaderValue, ACCEPT}, Client, StatusCode, Url};
use semver::Version;
use serde::Deserialize;
use clap::Parser;
use tokio::task::JoinSet;

pub type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    repositories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageTagList {
    // name: String,
    tags: Vec<String>,
}

/// Mark things for deletion, you'll have to run the garbage collection yourself
#[derive(Debug, Clone, Parser)]
#[command(version)]
pub struct Args {
    /// The base URL of the container registry. e.g. https://docker.io/
    #[arg(short, long)]
    registry_url: Url,
    /// Optional username to use when logging in to the registry.
    #[arg(long)]
    registry_user: Option<String>,
    /// Optional password to use when logging in to the registry.
    #[arg(long)]
    registry_password: Option<String>,
    /// Maximum number of images to keep per tag and regex pattern
    #[arg(short, long)]
    max_per_tag: NonZeroUsize,
    /// Regex for tag whitelist, multiple can be specified if any match then it's in whitelist. If none, no action is taken
    /// The max_per_tag is applied per pattern here. Specifying two will result in two separate
    /// lists of tags for max_per_tag.
    #[arg(short, long)]
    tags: Vec<String>,
    /// Regex for image whitelist, multiple can be specified if any of them match then it's in whitelist. If none all images are whitelisted
    #[arg(short, long)]
    images: Vec<String>,
    // TODO: Maybe an enum for things? Semver vs regex tags somewhat contradict each other if we
    // can't extract semver from the context.
    /// Should the tags be sorted by semver?
    #[arg(short,long)]
    semver: bool,
    /// Run actual deletions. Otherwise it's dry-run by default
    #[arg(short, long)]
    delete: bool,
}

pub async fn get_catalogs(
    client: &Client,
    registry_url: &Url,
    headers: &HeaderMap,
    auth: Option<&(String, Option<String>)>
) -> Result<Catalog, BoxError> {
    let mut catalog_request = client.get(registry_url.join("/v2/_catalog")?)
        .headers(headers.clone());

    if let Some(auth) = auth {
        catalog_request = catalog_request.basic_auth(&auth.0, auth.1.as_ref());
    }

    let catalog_response = catalog_request.send().await?.error_for_status()?;

    Ok(catalog_response.json().await?)
}

pub async fn get_tag_list(
    client: &Client,
    registry_url: &Url,
    headers: &HeaderMap,
    auth: Option<&(String, Option<String>)>,
    repository: &str,
) -> Result<ImageTagList, BoxError> {
    let mut tag_list_request = client.get(registry_url.join(&format!("/v2/{repository}/tags/list"))?)
        .headers(headers.clone());

    if let Some(auth) = auth {
        tag_list_request = tag_list_request.basic_auth(&auth.0, auth.1.as_ref());
    }

    let tag_list_response = tag_list_request.send().await?.error_for_status()?;

    Ok(tag_list_response.json().await?)
}

pub async fn get_tag_digest(
    client: &Client,
    registry_url: &Url,
    headers: &HeaderMap,
    auth: Option<&(String, Option<String>)>,
    repository: &str,
    tag: &str,
) -> Result<Option<String>, BoxError> {
    let mut tag_digest_request = client.head(registry_url.join(&format!("/v2/{repository}/manifests/{tag}"))?)
        .headers(headers.clone());

    if let Some(auth) = auth {
        tag_digest_request = tag_digest_request.basic_auth(&auth.0, auth.1.as_ref());
    }

    let tag_digest_response = tag_digest_request.send().await?;

    if tag_digest_response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let tag_digest_response = tag_digest_response.error_for_status()?;

    let tag_digest = tag_digest_response
        .headers()
        .iter()
        .find(|header| *header.0 == HeaderName::from_static("docker-content-digest"))
        .and_then(|header| header.1.to_str().ok())
        .map(|hv| hv.to_string());

    Ok(tag_digest)
}

pub async fn delete_tag(
    client: &Client,
    registry_url: &Url,
    headers: &HeaderMap,
    auth: Option<&(String, Option<String>)>,
    repository: &str,
    digest: &str,
) -> Result<(), BoxError> {
    let mut tag_delete_request = client.delete(registry_url.join(&format!("/v2/{repository}/manifests/{digest}"))?)
        .headers(headers.clone());

    if let Some(auth) = auth {
        tag_delete_request = tag_delete_request.basic_auth(&auth.0, auth.1.as_ref());
    }

    tag_delete_request.send().await?.error_for_status()?;

    Ok(())
}

/// Returns the pair of (tags_to_keep, tags_to_remove)
/// Will sort things in descending order. If semver == true, it'll use semantic versioning to order
/// the things. Otherwise it'll sort lexicographically.
pub fn classify_tags(mut tags: Vec<String>, num_tags: usize, semver: bool) -> (Vec<String>, Vec<String>) {
    let n = num_tags.min(tags.len());

    let sorted = if semver {
        let mut versions: Vec<(Version, String)> = tags
            .into_iter()
            .filter_map(|tag|{
                let vstr = tag.trim_start_matches('v');
                Version::parse(vstr).ok().map(|ver| (ver, tag))
            })
        .collect();

        versions.sort_unstable_by(|(a, _), (b, _)| b.cmp(a));
        versions.into_iter().map(|v| v.1).collect()
    } else {
        tags.sort_unstable_by(|a, b| b.cmp(a));

        tags
    };

    let tags_to_keep = sorted[..n].to_vec();
    let tags_to_remove = sorted[n..].to_vec();

    (tags_to_keep, tags_to_remove)
}

// TODO: How to handle overlap? I.e. regex .* and ^dev-.* both match dev
/// Sort all of the given tags into a hashmap based on the provided regex
pub fn get_matching_tags(tag_list: &ImageTagList, regex_tags: &Vec<(String, Regex)>) -> HashMap<String, Vec<String>> {
    let mut matching_tags: HashMap<String, Vec<String>> = HashMap::new();

    if regex_tags.is_empty() {
        matching_tags.insert(String::from(".*"), tag_list.tags.clone());
    } else {
        for tag in tag_list.tags.iter() {
            for regex_tag in regex_tags.iter() {
                if regex_tag.1.is_match(&tag) {
                    let e = matching_tags.entry(regex_tag.0.clone()).or_default();
                    e.push(tag.clone());
                }
            }
        }
    }

    matching_tags
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    // TODO: Write testable code.
    let args = Args::parse();

    let headers = HeaderMap::from_iter([
        (ACCEPT, HeaderValue::from_static("application/json,application/vnd.docker.distribution.manifest.v2+json,application/vnd.oci.image.manifest.v1+json"))
    ].into_iter());

    let auth = if let Some(username) = args.registry_user {
        Some((username, args.registry_password))
    } else {
        None
    };

    let regex_tags: Result<Vec<(String, Regex)>, regex::Error> = args.tags.iter().map(|t| Regex::new(t).map(|r| (t.clone(), r))).collect();
    let regex_tags = match regex_tags {
        Ok(v) => v,
        Err(e) => {
            return Err(format!("Invalid tag regex: {e}").into());
        }
    };

    let regex_images: Result<Vec<(String, Regex)>, regex::Error> = args.images.iter().map(|t| Regex::new(t).map(|r| (t.clone(), r))).collect();
    let regex_images = match regex_images {
        Ok(v) => v,
        Err(e) => {
            return Err(format!("Invalid tag regex: {e}").into());
        }
    };

    let client = Client::new();

    let catalog_data: Catalog = get_catalogs(&client, &args.registry_url, &headers, auth.as_ref()).await?;

    let mut join_set: JoinSet<Result<(Vec<String>, usize), BoxError>> = JoinSet::new();

    for repository in catalog_data.repositories {
        if !regex_images.is_empty() && !regex_images.iter().any(|regexp| regexp.1.is_match(&repository)) {
            println!("Image doesn't match any of the images specified.");
            continue;
        }

        let client = client.clone();
        let registry_url = args.registry_url.clone();
        let headers = headers.clone();
        let auth = auth.clone();
        let regex_tags = regex_tags.clone();

        join_set.spawn(async move {
            let tag_list = get_tag_list(&client, &registry_url, &headers, auth.as_ref(), &repository).await?;
            let matching_tags = get_matching_tags(&tag_list, &regex_tags);

            let mut log: Vec<String> = Vec::new();
            let mut tags_for_deletion = 0;

            if matching_tags.is_empty() {
                log.push(format!("[{repository}] No tags eligable for deletion found."));
            } else {
                for t in matching_tags.into_iter() {
                    // TODO: Make testable?
                    // TODO: Decide sort order?
                    // TODO: Allow specifying ways to sort? Kinda like how it's done by flux image policies?
                    let (_tags_to_keep, tags_to_remove) = classify_tags(t.1, args.max_per_tag.into(), args.semver);

                    if !tags_to_remove.is_empty() {
                        tags_for_deletion += tags_to_remove.len();
                        log.push(format!("[{repository}] Found {} tags eligable for deletion for pattern /{}/", tags_to_remove.len(), t.0));

                        for tag_to_remove in tags_to_remove {
                            let tag_digest = get_tag_digest(&client, &registry_url, &headers, auth.as_ref(), &repository, &tag_to_remove).await?;

                            if let Some(tag_digest) = tag_digest {
                                log.push(format!("[{repository}] tag to be deleted {tag_to_remove}"));

                                if args.delete {
                                    delete_tag(&client, &registry_url, &headers, auth.as_ref(), &repository, &tag_digest).await?;
                                    log.push(format!("[{repository}] Deleted {tag_to_remove}"))
                                }
                            } else {
                                log.push(format!("[{repository}] WARNING: Couldn't find tag digest for {tag_to_remove}"));
                            }
                        }
                    }
                }

                if tags_for_deletion == 0 {
                    log.push(format!("[{repository}] No tags eligable for deletion found."));
                }
            }

            Ok((log, tags_for_deletion))
        });
    }

    let mut num_tags_to_delete: usize = 0;
    let mut errors = Vec::new();

    while let Some(thread_result) = join_set.join_next().await {
        match thread_result? {
            Ok((log, n)) => {
                println!("{}", log.join("\n"));
                num_tags_to_delete += n;
            },
            Err(e) => errors.push(e),
        }
    }

    if !errors.is_empty() {
        println!("The following errors occured during processing:\n\t{}\n", errors.into_iter().map(|e| format!("{e}")).collect::<Vec<String>>().join("\n\t"));
    }

    if args.delete {
        println!("\n\tDeleted a total of {num_tags_to_delete} tag(s)");
        println!("\n\tRemember to run garbage collection on your registry to ensure that files get removed on disk.");
    } else {
        println!("\n\tFound a total of {num_tags_to_delete} tag(s) to delete");
        println!("\n\tDelete flag (-d/--delete) not specified, none of the above have actually been deleted.");
    }

    Ok(())
}

