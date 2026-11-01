use reddit_offline::{media, reddit_html, response_capture::CapturedResponse, Thread};

#[test]
fn streamin_goal_clip_is_video_and_takes_precedence_over_preview() {
    for host in ["streamin.link", "streamin.me"] {
        let mut thread = post(&format!("https://{host}/v/0b149d9a"), "");
        thread.media_urls = vec!["https://preview.redd.it/poster.jpg?s=signed".into()];
        assert_eq!(
            media::sources(&thread),
            [format!("https://{host}/v/0b149d9a")]
        );
        assert_eq!(media::hint(&thread).unwrap().kind, "video");
    }
    assert!(media::media_host("c-cdn.streamin.top"));
    assert!(!media::media_host("streamin.link.attacker.example"));
    assert!(!media::media_host("notstreamin.link"));
}

#[cfg(unix)]
#[tokio::test]
async fn streamin_link_reaches_ytdlp_with_bounded_parallel_fragments_and_reuses_cache() {
    use std::os::unix::fs::PermissionsExt;
    let root =
        std::env::temp_dir().join(format!("hrungnir-streamin-helper-{}", std::process::id()));
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let helper = tools.join("yt-dlp");
    std::fs::write(
        &helper,
        r#"#!/usr/bin/env python3
import sys,json,pathlib
args=sys.argv[1:]
pathlib.Path(__file__).with_name('args.json').write_text(json.dumps(args))
output=args[args.index('--output')+1].replace('%(ext)s','mp4')
pathlib.Path(output).write_bytes(b'synthetic helper output')
"#,
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let thread = post("https://streamin.link/v/0b149d9a", "");
    let assets = media::download(root.join("cache"), tools.clone(), &thread)
        .await
        .unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].kind, "video");
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(tools.join("args.json")).unwrap()).unwrap();
    assert_eq!(args.last().unwrap(), "https://streamin.link/v/0b149d9a");
    assert!(args
        .windows(2)
        .any(|a| a == ["--concurrent-fragments", "4"]));
    assert!(args.windows(2).any(|a| a == ["--fragment-retries", "1"]));
    assert!(args.iter().any(|a| a == "--ignore-config"));
    assert!(!args.iter().any(|a| a.starts_with("--cookies")));
    std::fs::remove_file(helper).unwrap();
    assert_eq!(
        media::download(root.join("cache"), tools, &thread)
            .await
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn post(link: &str, content: &str) -> Thread {
    let html = format!(
        r#"<div class="thing link" data-fullname="t3_test">
      <a class="title" href="{link}">A public post</a>
      <a class="thumbnail" href="{link}"><img src="https://external-preview.redd.it/article.jpeg?width=140&amp;s=thumbnail-signature"></a>
      <span class="flair"><img src="https://i.redd.it/flair.png"></span>
      <a class="comments empty" href="/r/soccer/comments/test/title/">0 comments</a>
      {content}</div>"#
    );
    reddit_html::parse(CapturedResponse {
        generation: 0,
        request_id: 0,
        request_cursor_fingerprint: None,
        status_code: 200,
        content_type_kind: "html",
        url: "https://www.reddit.com/r/soccer/comments/test/title/".into(),
        encoding: String::new(),
        bytes: html.into_bytes(),
    })
    .unwrap()
    .threads
    .remove(0)
}

#[test]
fn external_article_thumbnail_and_flair_are_not_required_media() {
    let thread = post("https://news.example/article", "");
    assert_eq!(
        thread.link_url.as_deref(),
        Some("https://news.example/article")
    );
    assert!(thread.media_urls.is_empty());
    assert!(media::sources(&thread).is_empty());
    assert!(media::hint(&thread).is_none());
}

#[tokio::test]
async fn old_cached_article_thumbnails_are_ignored_without_network_or_helpers() {
    let mut thread = post("https://news.example/article", "");
    thread.media_urls = vec![
        "https://external-preview.redd.it/article.jpeg?s=signed".into(),
        "https://b.thumbs.redditmedia.com/thumb.jpg".into(),
        "https://www.redditstatic.com/icon.png".into(),
    ];
    assert!(media::sources(&thread).is_empty());
    assert!(media::hint(&thread).is_none());
    let root = std::env::temp_dir().join(format!("hrungnir-article-media-{}", std::process::id()));
    let assets = media::download(root.clone(), root.join("missing-tools"), &thread)
        .await
        .unwrap();
    assert!(assets.is_empty());
    assert!(media::cached_assets(&root, &thread).is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_image_queries_survive_html_and_candidate_selection() {
    let thread = post(
        "https://www.reddit.com/r/soccer/comments/test/title/",
        r#"
      <div class="expando"><div class="usertext-body"><div class="md">
        <img src="https://preview.redd.it/photo.jpg?width=1080&amp;format=pjpg&amp;s=keep%2Bsignature">
      </div></div></div>"#,
    );
    let expected = "https://preview.redd.it/photo.jpg?width=1080&format=pjpg&s=keep%2Bsignature";
    assert_eq!(thread.media_urls, [expected]);
    assert_eq!(media::sources(&thread), [expected]);
    let hint = media::hint(&thread).unwrap();
    assert_eq!(hint.kind, "image");
    assert_eq!(hint.count, 1);
    let json = serde_json::to_string(&hint).unwrap();
    assert!(!json.contains("signature") && !json.contains("https"));

    let direct = post("https://i.redd.it/original.png?token=preserved", "");
    assert_eq!(
        media::sources(&direct),
        ["https://i.redd.it/original.png?token=preserved"]
    );
    assert_eq!(media::hint(&direct).unwrap().kind, "image");
}

#[test]
fn cached_expando_video_and_supported_video_links_keep_video_hint() {
    let thread = post(
        "https://v.redd.it/clip123?f=hd",
        r#"
      <div class="expando" data-cachedhtml="&lt;video&gt;&lt;source src=&quot;https://v.redd.it/clip123/HLSPlaylist.m3u8?f=hd&amp;amp;token=keep&quot;&gt;&lt;/video&gt;"></div>"#,
    );
    assert_eq!(
        thread.media_urls,
        ["https://v.redd.it/clip123/HLSPlaylist.m3u8?f=hd&token=keep"]
    );
    assert_eq!(media::sources(&thread), thread.media_urls);
    let hint = media::hint(&thread).unwrap();
    assert_eq!(hint.kind, "video");
    assert_eq!(hint.count, 1);
    for link in [
        "https://v.redd.it/clip123",
        "https://www.youtube.com/watch?v=clip",
        "https://streamable.com/clip",
    ] {
        assert_eq!(media::hint(&post(link, "")).unwrap().kind, "video");
    }
}
