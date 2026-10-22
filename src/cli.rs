//! Public headless CLI, shared by the standalone core and packaged app.
use crate::{
    organization::{SavedItem, SubredditGroup},
    Cache, Result, Thread,
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

const HELP: &str = "Hrungnir --cli [--data-dir DIR] COMMAND\n\
  library list | search QUERY | save ID [COLLECTION] | remove ID\n\
  library save-url URL article|video|image [COLLECTION]\n\
  library export FILE.zip | import FILE.zip | export-links FILE.json\n\
  import links FILE.json|FILE.html [COLLECTION]  (bookmarks only; no downloads)\n\
  groups list | put FILE.json | delete ID\n\
  search QUERY | read ID | cache settings [HOURS] | doctor\n\
  diagnostics export FILE.zip\n\
Saved library items default to keep forever. Downloads occur only for an explicit save-url.\n";

pub fn default_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HRUNGNIR_DATA_DIR") {
        return Ok(path.into());
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set; use --data-dir")?;
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(home).join("Library/Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".local/share"));
    Ok(base.join("io.github.iarnvidr.hrungnir"))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Import exceeds 4 MiB".into());
    }
    Ok(bytes)
}

fn export_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub fn run(mut args: Vec<String>, tools: PathBuf) -> Result<()> {
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h" | "help") {
        print!("{HELP}");
        return Ok(());
    }
    let root = if args.first().map(String::as_str) == Some("--data-dir") {
        if args.len() < 3 {
            return Err(HELP.into());
        }
        let path = PathBuf::from(args.remove(1));
        args.remove(0);
        path
    } else {
        default_root()?
    };
    std::fs::create_dir_all(&root)?;
    let cache = Cache::open(root.join("cache.sqlite"))?;
    cache.expire_post_cache((crate::diagnostics::now_ms() / 1000) as i64)?;
    let strings: Vec<_> = args.iter().map(String::as_str).collect();
    let value = match strings.as_slice() {
        ["library", "list"] => crate::library::state(&root)?,
        ["library", "search", query] => {
            cache.expire_post_cache((crate::diagnostics::now_ms() / 1000) as i64)?;
            serde_json::to_value(cache.search_library(query, 100)?)?
        }
        ["library", "save", id] | ["library", "save", id, _] => {
            let collection = strings.get(3).copied().unwrap_or("Library");
            serde_json::to_value(cache.save_item(id, "post", collection, true)?)?
        }
        ["library", "remove", id] => {
            cache.remove_saved_item(id)?;
            serde_json::json!({"removed":id,"files_deleted":false})
        }
        ["library", "save-url", url, kind] | ["library", "save-url", url, kind, _] => {
            let collection = strings.get(4).copied().unwrap_or("Library");
            let id = tokio::runtime::Runtime::new()?
                .block_on(crate::linked_content::save(
                    &root, &tools, url, kind, collection, true,
                ))
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            serde_json::json!({"saved":id,"keep_forever":true})
        }
        ["library", "export", file] => crate::library_bundle::export(&root, Path::new(file))?,
        ["library", "import", file] => crate::library_bundle::import(&root, Path::new(file))?,
        ["library", "export-links", file] => {
            let mut links = Vec::new();
            for item in cache.saved_items()? {
                if let Some(thread) = cache.read(&item.id)? {
                    links.push(serde_json::json!({"url":thread.link_url.or(thread.permalink),"title":thread.title,"collection":item.collection,"kind":item.kind}));
                }
            }
            export_json(Path::new(file), &serde_json::json!({"items":links}))?;
            serde_json::json!({"exported":links.len(),"path":file})
        }
        ["import", "links", file] | ["import", "links", file, _] => import_links(
            &cache,
            Path::new(file),
            strings.get(3).copied().unwrap_or("Imported"),
        )?,
        ["groups", "list"] => serde_json::to_value(cache.groups()?)?,
        ["groups", "put", file] => {
            let group: SubredditGroup = serde_json::from_slice(&read_bounded(Path::new(file))?)?;
            cache.put_group(&group)?;
            serde_json::to_value(cache.groups()?)?
        }
        ["groups", "delete", id] => {
            cache.delete_group(id)?;
            serde_json::to_value(cache.groups()?)?
        }
        ["search", query] => {
            cache.expire_post_cache((crate::diagnostics::now_ms() / 1000) as i64)?;
            serde_json::to_value(cache.search(query, None, 100)?)?
        }
        ["read", id] => serde_json::to_value(cache.read(id)?.ok_or("Post is not cached")?)?,
        ["cache", "settings"] => serde_json::json!({"post_cache_hours":cache.post_cache_hours()?}),
        ["cache", "settings", hours] => {
            cache.set_post_cache_hours(hours.parse()?)?;
            cache.expire_post_cache((crate::diagnostics::now_ms() / 1000) as i64)?;
            serde_json::json!({"post_cache_hours":cache.post_cache_hours()?})
        }
        ["doctor"] => cache.diagnostics()?,
        ["diagnostics", "export", file] => {
            crate::diagnostics::export(&root, Path::new(file), None)?
        }
        _ => return Err(HELP.into()),
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn import_links(cache: &Cache, file: &Path, collection: &str) -> Result<serde_json::Value> {
    let bytes = read_bounded(file)?;
    let text = std::str::from_utf8(&bytes)?;
    let links: Vec<(String, String)> = if text.trim_start().starts_with('<') {
        let html = scraper::Html::parse_document(text);
        html.select(&scraper::Selector::parse("a[href]").unwrap())
            .map(|a| {
                (
                    a.value().attr("href").unwrap().to_owned(),
                    a.text().collect::<Vec<_>>().join(" "),
                )
            })
            .collect()
    } else {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let items = value
            .as_array()
            .or_else(|| value.get("items").and_then(|items| items.as_array()))
            .ok_or("Expected an array or {items:[url or {url,title}]} JSON")?;
        items
            .iter()
            .map(|item| {
                if let Some(url) = item.as_str() {
                    Ok((url.to_owned(), url.to_owned()))
                } else {
                    let url = item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .ok_or("Link has no URL")?;
                    Ok((
                        url.to_owned(),
                        item.get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or(url)
                            .to_owned(),
                    ))
                }
            })
            .collect::<Result<_>>()?
    };
    if links.len() > 10000 {
        return Err("At most 10000 bookmarks per import".into());
    }
    let now = (crate::diagnostics::now_ms() / 1000) as i64;
    let mut threads = Vec::new();
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (url, title) in links {
        let url = crate::linked_content::public_url(&url)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        let hash = url.as_str().bytes().fold(0xcbf29ce484222325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        let id = format!("t3_bookmark{hash:016x}");
        if !seen.insert(id.clone()) {
            continue;
        }
        if cache.read(&id)?.is_none() {
            threads.push(Thread{id:id.clone(),subreddit:"saved_links".into(),title:title.chars().take(1000).collect(),body:"Saved bookmark. Article/media has not been downloaded. Use Save linked content to download this item.".into(),captured_at:now,comments:vec![],missing:vec![],permalink:None,link_url:Some(url.to_string()),thumbnail_url:None,source:Some("linked_bookmark".into()),comments_status:Some("empty_confirmed".into()),media_urls:vec![]});
        }
        items.push(SavedItem {
            id,
            kind: "article".into(),
            collection: collection.into(),
            keep_forever: true,
            saved_at: now,
        });
    }
    cache.import_library(&threads, &[], &items)?;
    Ok(serde_json::json!({"imported":items.len(),"downloads_started":0,"collection":collection}))
}
