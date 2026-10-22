//! Shared library view used by the desktop commands and CLI.
use crate::{Cache, Result};
use std::path::Path;

pub fn state(root: &Path) -> Result<serde_json::Value> {
    let cache = Cache::open(root.join("cache.sqlite"))?;
    cache.expire_post_cache((crate::diagnostics::now_ms() / 1000) as i64)?;
    let mut items = Vec::new();
    for item in cache.saved_items()? {
        let Some(mut thread) = cache.read(&item.id)? else {
            continue;
        };
        let media = crate::linked_content::cached_assets(&root.join("media"), &thread)
            .or_else(|| crate::media::cached_assets(&root.join("media"), &thread))
            .unwrap_or_default();
        let mut value = serde_json::to_value(item)?;
        value["cached_comments"] = serde_json::json!(thread.comments.len());
        thread.body = thread.body.chars().take(2000).collect();
        thread.comments.clear();
        thread.missing.clear();
        value["thread"] = serde_json::to_value(thread)?;
        value["media"] = serde_json::to_value(media)?;
        items.push(value);
    }
    Ok(serde_json::json!({"groups":cache.groups()?,"items":items}))
}
