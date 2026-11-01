//! Safe merge rules for bounded Reddit comment-page deltas.
//!
//! Source cursors are intentionally not part of this persisted model. The
//! caller keeps them in short-lived acquisition state and only writes parsed,
//! validated comment records here.

use crate::{Comment, Result, Thread};
use std::collections::{HashMap, HashSet};

pub const MAX_PAGE_COMMENTS: usize = 300;

/// Merge one source page into a cached discussion. Fresh representations win
/// for repeated IDs without moving existing comments; new IDs append in the
/// source's order. Every returned item must descend from the requested parent.
pub fn merge_page(thread: &mut Thread, requested_parent: &str, page: Vec<Comment>) -> Result<usize> {
    if page.len() > MAX_PAGE_COMMENTS {
        return Err("comment page exceeds 300 comments".into());
    }
    if requested_parent != thread.id
        && !thread.comments.iter().any(|comment| comment.id == requested_parent)
    {
        return Err("comment page parent is not in the cached discussion".into());
    }

    let mut fresh_by_id = HashMap::new();
    let mut fresh_order = Vec::new();
    for comment in page {
        if comment.id == requested_parent
            || !comment.id.starts_with("t1_")
            || comment.id.len() > 32
            || !comment.id[3..].bytes().all(|byte| byte.is_ascii_alphanumeric())
            || comment.body.chars().count() > 3000
            || comment.parent_id != thread.id
                && (!comment.parent_id.starts_with("t1_")
                    || comment.parent_id.len() > 32
                    || !comment.parent_id[3..]
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric()))
        {
            return Err("comment page contains an invalid comment identity".into());
        }
        if !fresh_by_id.contains_key(&comment.id) {
            fresh_order.push(comment.id.clone());
        }
        fresh_by_id.insert(comment.id.clone(), comment);
    }

    let mut known: HashSet<String> = thread
        .comments
        .iter()
        .map(|comment| comment.id.clone())
        .collect();
    known.extend(fresh_by_id.keys().cloned());
    for comment in fresh_by_id.values() {
        let mut ancestor = comment.parent_id.as_str();
        let mut seen = HashSet::new();
        let mut descended_from_requested_parent = ancestor == requested_parent;
        while ancestor != thread.id {
            if !seen.insert(ancestor) || !known.contains(ancestor) {
                return Err("comment page contains an unresolved or cyclic parent".into());
            }
            if ancestor == requested_parent {
                descended_from_requested_parent = true;
            }
            ancestor = fresh_by_id
                .get(ancestor)
                .map(|comment| comment.parent_id.as_str())
                .or_else(|| {
                    thread
                        .comments
                        .iter()
                        .find(|existing| existing.id == ancestor)
                        .map(|comment| comment.parent_id.as_str())
                })
                .ok_or("comment page contains an unresolved parent")?;
        }
        // The loop stops at the thread itself. For a root-page request that
        // terminal ancestor is the requested parent, even though it is not an
        // ordinary comment ID that the loop can visit.
        if ancestor == requested_parent {
            descended_from_requested_parent = true;
        }
        if !descended_from_requested_parent {
            return Err("comment page returned a comment outside the requested branch".into());
        }
    }

    let mut new_ids = 0;
    for comment in &mut thread.comments {
        if let Some(fresh) = fresh_by_id.remove(&comment.id) {
            *comment = fresh;
        }
    }
    for id in fresh_order {
        if let Some(comment) = fresh_by_id.remove(&id) {
            new_ids += 1;
            thread.comments.push(comment);
        }
    }
    if thread.comments.len() > 10_000 {
        return Err("merged discussion exceeds 10000 cached comments".into());
    }
    let loaded: HashSet<&str> = thread.comments.iter().map(|comment| comment.id.as_str()).collect();
    thread.missing.retain(|id| !loaded.contains(id.as_str()));
    if !thread.comments.is_empty() && thread.comments_status.is_none() {
        thread.comments_status = Some("partial_proxy".into());
    }
    thread.validate()?;
    Ok(new_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread() -> Thread {
        Thread {
            id: "t3_abc123".into(),
            subreddit: "AskReddit".into(),
            title: "question".into(),
            body: String::new(),
            captured_at: 1,
            comments: vec![Comment {
                id: "t1_parent".into(),
                parent_id: "t3_abc123".into(),
                body: "old body".into(),
                author: Some("old".into()),
                author_flair: None,
            }],
            missing: vec!["t1_new".into(), "t1_stillmissing".into()],
            permalink: Some("https://www.reddit.com/r/AskReddit/comments/abc123/x/".into()),
            link_url: None,
            thumbnail_url: None,
            flair: None,
            source: Some("proxy".into()),
            comments_status: Some("partial_proxy".into()),
            comment_count: Some(900),
            media_urls: Vec::new(),
        }
    }

    #[test]
    fn page_merge_updates_known_comments_and_appends_new_descendants() {
        let mut thread = thread();
        let added = merge_page(
            &mut thread,
            "t1_parent",
            vec![
                Comment {
                    id: "t1_new".into(),
                    parent_id: "t1_parent".into(),
                    body: "fresh".into(),
                    author: None,
                    author_flair: None,
                },
                Comment {
                    id: "t1_grandchild".into(),
                    parent_id: "t1_new".into(),
                    body: "nested".into(),
                    author: None,
                    author_flair: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(added, 2);
        assert_eq!(thread.comments[0].id, "t1_parent");
        assert_eq!(thread.comments[1].id, "t1_new");
        assert_eq!(thread.comments[2].parent_id, "t1_new");
        assert_eq!(thread.missing, vec!["t1_stillmissing"]);
        assert_eq!(thread.comment_count, Some(900));
        assert_eq!(thread.comments_status.as_deref(), Some("partial_proxy"));
    }

    #[test]
    fn repeated_page_ids_refresh_in_place_without_reordering() {
        let mut thread = thread();
        let added = merge_page(
            &mut thread,
            "t3_abc123",
            vec![Comment {
                id: "t1_parent".into(),
                parent_id: "t3_abc123".into(),
                body: "fresh body".into(),
                author: Some("new".into()),
                author_flair: None,
            }],
        )
        .unwrap();
        assert_eq!(added, 0);
        assert_eq!(thread.comments.len(), 1);
        assert_eq!(thread.comments[0].body, "fresh body");
    }

    #[test]
    fn root_page_accepts_inline_descendants_through_thread_ancestor() {
        let mut thread = thread();
        let added = merge_page(
            &mut thread,
            "t3_abc123",
            vec![
                Comment {
                    id: "t1_rootnew".into(),
                    parent_id: "t3_abc123".into(),
                    body: "top-level".into(),
                    author: None,
                    author_flair: None,
                },
                Comment {
                    id: "t1_childnew".into(),
                    parent_id: "t1_rootnew".into(),
                    body: "inline child".into(),
                    author: None,
                    author_flair: None,
                },
                Comment {
                    id: "t1_grandnew".into(),
                    parent_id: "t1_childnew".into(),
                    body: "inline grandchild".into(),
                    author: None,
                    author_flair: None,
                },
            ],
        )
        .unwrap();

        assert_eq!(added, 3);
        assert_eq!(
            thread.comments.iter().map(|comment| comment.id.as_str()).collect::<Vec<_>>(),
            vec!["t1_parent", "t1_rootnew", "t1_childnew", "t1_grandnew"]
        );
        assert_eq!(thread.comments[3].parent_id, "t1_childnew");
    }

    #[test]
    fn page_merge_rejects_foreign_or_unresolved_branches() {
        let mut thread = thread();
        let before = thread.comments.clone();
        let wrong_branch = vec![
            Comment {
                id: "t1_otherroot".into(),
                parent_id: "t3_abc123".into(),
                body: "unrelated top-level".into(),
                author: None,
                author_flair: None,
            },
            Comment {
                id: "t1_otherchild".into(),
                parent_id: "t1_otherroot".into(),
                body: "unrelated nested reply".into(),
                author: None,
                author_flair: None,
            },
        ];
        assert!(merge_page(&mut thread, "t1_parent", wrong_branch).is_err());
        assert_eq!(thread.comments, before);

        let bad = Comment {
            id: "t1_foreign".into(),
            parent_id: "t3_other".into(),
            body: "foreign".into(),
            author: None,
            author_flair: None,
        };
        assert!(merge_page(&mut thread, "t1_parent", vec![bad]).is_err());
        let bad = Comment {
            id: "t1_missingparent".into(),
            parent_id: "t1_unknown".into(),
            body: "unresolved".into(),
            author: None,
            author_flair: None,
        };
        assert!(merge_page(&mut thread, "t1_parent", vec![bad]).is_err());
    }
}
