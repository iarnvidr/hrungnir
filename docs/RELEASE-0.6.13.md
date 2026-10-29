# Hrungnir 0.6.13

- Fixes fetching: Reddit now redirects `old.reddit.com` and no longer honors the classic-layout preference on `www.reddit.com`, so every subreddit, post and search fetch failed. Hrungnir now reads Reddit's current server-rendered layout (listings, posts with comments, search, images, galleries and video links) from `www.reddit.com`; classic markup remains supported as a fallback.
- Reddit's first-request verification page and posts opened without their title slug are waited through instead of failing, and the verification fields are never stored in captured addresses.
- A subreddit listing is rendered with only a few posts, so Hrungnir also requests the read-only continuation Reddit's own page uses and merges up to 25 posts. If that request fails, the listing shows the posts that did arrive after a short bounded wait.
- Known limits: a post captures the comments present in the first response (typically about 25); further comments load through a request Hrungnir does not make. Comment author flair is not extracted from the new layout.

Apple Silicon, macOS 14+. Update through Settings → Check for updates, or download the ZIP and verify its checksum before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.13-macos-arm64.zip.sha256
```

Validation: Rust tests parse structures taken from live www.reddit.com listing, post, gallery, search and continuation responses captured on 2026-10-29, including the verification page and unslugged-post shell; a Chromium probe confirmed the verification, slug redirect and continuation behavior. GitHub Actions builds the native macOS app and verifies the release artifacts. The hidden WebKit worker has not been exercised against live Reddit outside that build.
