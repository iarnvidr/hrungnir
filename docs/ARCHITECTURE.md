# Architecture — download before display

Updated 18 October 2026.

## Media reuse and publication (0.6.6)

Media downloads use private staging with cleanup on failure. Playable files and the manifest are validated before actual-size budget eviction and publication. No worst-case reservation evicts existing content before a helper succeeds. The evictable completed-media budget remains 512 MiB; protected content is outside it and leases prevent removal. Successful reuse updates directory recency for LRU only; manifest/media timestamps continue to identify acquisition and watch coverage. Known bare/HLS/DASH representations of one v.redd.it clip share a cache identity; different clip IDs and unknown paths stay distinct. Regenerable poster loss is repaired locally and does not invalidate playable media.

Listing/search snapshots cannot replace full cached discussions or their media identities with summaries. This includes confirmed-empty discussions. Detail captures remain authoritative. Native download responses include separate comments/media reuse flags, including a second cache check after waiting for the media gate. The local reader therefore distinguishes actual reuse from an acquisition request that happened to return local files. Diagnostics events `post.cache`, `post.cache_validation`, `post.cache_result`, `media.cache_miss` and `media.budget_eviction` explain subsequent acquisition decisions without logging signed source URLs in the new reason fields.

## Reddit search, previews and external links

Header input immediately queries local FTS, showing cached matches without network acquisition. Selecting a match opens a cached-only tab and reuses existing previews without downloading missing thumbnails. Submitting the same field sends its query to the existing hidden classic worker for live Reddit results. Submission invalidates pending local responses so they cannot cover live results; no scope selector is required. Editing the query invalidates older live-result UI authority. The proxy captures allowlisted public `/search/` and `/r/<name>/search/` HTML routes. The Rust parser validates each post permalink, identity and community; global results can span communities while scoped results must match. Query/sort/period are preserved in active-request validation. Empty live results stay empty, without substituting cached content. Search results keep their source order independently of subreddit listings and post snapshots.

Classic listing/search thumbnails use an independent 64 MiB derivative cache. At most four bounded 512 KiB raster downloads run concurrently, limited to Reddit image CDNs; redirects stay within that allowlist. The local reader receives only tokenized loopback URLs. Preview completion patches row previews without replacing active video players. The existing media/watch store and Library remain independent.

Inline articles use the existing public-HTTPS readable-text extractor and private-address DNS rejection. A separate 32 MiB text cache follows the post/group lifetime and does not create a saved Library item. It is a disposable derivative cache; permanent Library saves remain the durable path. Article bodies and detected links are rendered as text, never remote HTML.

Explicit **Open link** creates a separate incognito native webview with ordinary TLS and no Reddit proxy. It has no reader capability grant; public HTTPS top-level destinations are validated, and new-window requests reuse that isolated window. Back/forward/reload use the native Navigate menu; close-tab closes the focused link window. This is online browsing, separate from offline article extraction. Native Linux visual rendering was checked; WebKitGTK WebDriver does not expose this incognito automation context, so remote-page command denial is established by capability configuration rather than a live remote-page IPC test. Native macOS behavior remains pending.

## Preview and acquisition retention (0.6.2)

Local FFmpeg generates one bounded JPEG from the first video frame. The derivative is listed in the completion manifest for loopback serving and library backups, but excluded from playable assets and watch coverage. Existing media upgrades locally when selected; the manifest's original modification timestamp is preserved. Preview failures are logged separately from playable-file completion. No remote preview URL or autoplay is used.

`media/watch.sqlite` also records per-file acquisition time and fingerprint. Unwatched files inherit the group's post-cache lifetime or global default (48 hours), starting at completed download. Legacy caches bootstrap once from their completion marker time. Fully watched temporal media uses its separate completion clock and group/default 24-hour delay instead. Reuse never restarts either clock; protected saves and leases still prevent expiry. Preview images do not create their own expiry clock.

## Durable organization and saved content (0.6.0)

Subreddit groups and saved-item metadata live in the shared content SQLite database. One group owns each subreddit; post lifetime and watched-media delay inherit independently, with nullable forever policies. Saved `keep_forever` rows protect text and completed media from TTL, budget eviction and cache clearing. Frozen completed post snapshots resist later automatic proxy/browser replacements; explicit backup import remains a deliberate replacement. Temporary saved metadata is removed when its cached thread expires. The watch database remains separate; its original completion timestamps are evaluated against current group policy, while active-player leases still prevent deletion.

Protected text/media are excluded from evictable budgets. Unlike Muninn's physical `library/` media directory, Hrungnir currently retains protected and evictable files under the same independent `media/` tree and enforces protection through durable SQLite metadata. Subreddit mappings persist for media even after an unprotected post expires. Policy queries use the indexed mapping table rather than repeatedly parsing large cached discussion JSON.

The Library API sends summaries and local asset metadata; opening an item loads complete content from disk. Default browsing/startup snapshots also omit full comments and truncate body previews. Selected-post snapshots remain complete. Saved-only FTS filters membership before ranking/limiting, searches full bodies/comments, and retains comment anchors. Standalone linked items do not appear as synthetic subreddits in browsing.

Explicit linked saves use a bounded HTTPS article/image downloader or bundled yt-dlp for selected supported video providers. Readable article text and up to six optional images are stored locally; remote HTML is not rendered. These saved objects use the common thread/FTS model with `source=linked`, plus typed Library metadata and completed manifests. Imported bookmark lists use `linked_bookmark` and never trigger downloads. See [Library](LIBRARY.md) for extraction and size bounds.

Portable library ZIPs contain saved structured content, groups, organization and completed files, excluding raw databases, browser state and keys. Imports validate/stage all entries, commit metadata transactionally and reject conflicting files. Existing keep protection is preserved. GUI operations serialize through the media gate; CLI archive writes should run with the app closed for a consistent filesystem snapshot. A shared headless CLI runs before GUI initialization and uses the same policies and data. See [CLI](CLI.md).

## Local reader and background acquisition

The primary surface is the bundled classic-style reader; explicitly opened external links have a separate isolated webview. Fetching a subreddit downloads structured listing data before displaying titles. Selecting a post awaits comment acquisition and all supported media downloads before replacing the reader content. The UI exposes loading, failure and Retry states; it never offers a modern Reddit browser, Live/Saved modes or remote media playback in the reader.

An invisible Tauri webview makes public requests through the per-launch loopback HTTPS capture proxy. The worker uses `www.reddit.com` only: `old.reddit.com` now redirects and the public `redesign_optout=true` preference is no longer honored, so the worker still sets that cookie (harmless if Reddit ever honors it again) but the parser reads Reddit's current server-rendered `shreddit-*` markup, with the classic selectors kept as a fallback. Reddit first answers with a small proof-of-work page whose script resubmits the same address with `solution`, `js_challenge`, `jsc_token` and `jsc_orig_r` fields; the parser recognizes that page, and a post opened without its title slug, as interim responses that keep the acquisition waiting, and the verification fields are stripped from captured addresses. A server-rendered listing contains only a few posts, so the proxy also permits the read-only `GET /svc/shreddit/community-more-posts/{hot,best,new,top,rising}/?name=<subreddit>` fragment the page requests for itself; its posts are merged into the listing, capped at 25, and a missing fragment only means fewer posts. Native cookie-store completion precedes navigation. The proxy terminates TLS only for that exact host, verifies upstream TLS with public roots, and tees completed successful HTML responses while forwarding the original response frames to the worker. A Rust HTML parser runs off the proxy I/O path and commits validated structured snapshots to SQLite. Browser DOM export and the remote capture IPC capability have been removed.

This follows Muninn's response-capture pattern, including adapted streaming tee and app-scoped certificate trust code, with its MIT notice retained. A per-install private CA issues only the `www.reddit.com` leaf; roots have a matching DNS constraint. Key files are created with mode 0600. Linux approves the exact leaf for the exact hostname. The Mac delegate validates the chain against this CA as the sole trust anchor and otherwise leaves trust handling unchanged. Version 0.5.2 also provides optional login-Keychain SSL trust for `www.reddit.com`, behind explicit native confirmation; other apps in the same user account may use that host trust. No system/admin trust or automatic installation is used. Native Mac trust behavior remains unverified.

Non-community/private paths and write methods are refused by the decrypted forwarder. Ordinary permitted assets use uncaptured tunnels; source-webview Reddit media connections are refused so completed media acquisition stays independent. Proxy cancellation and capture acceptance use monotonically increasing acquisition generations: a short offline interval cannot retain an older connection, and a queued response cannot satisfy a later fetch of the same URL or a different Top period.

## macOS trust correction in 0.5.1

The 0.5.0 Mac support evidence showed rejected browser-to-proxy TLS handshakes before any HTTP request. [WebKit caches optional delegate callbacks when assigning the delegate](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/Cocoa/NavigationState.mm); adding the trust method afterward left the first worker's cached capability false. Hrungnir now retains the existing delegate and reassigns it after method installation, before navigation.

The shared `src-tauri/Info.plist` declares an exact `www.reddit.com` ATS exception for custom trust evaluation, without subdomains or global arbitrary-load settings. [Apple documents this exception as necessary for loosening server-trust evaluation](https://developer.apple.com/documentation/bundleresources/information-property-list/nsexceptionallowsinsecurehttploads). The code still requires HTTPS and validates only against the app's CA; public upstream TLS is unchanged. The packager and policy tests verify this narrow scope. No keychain trust is installed.

A bounded, generation-tagged TLS failure channel wakes acquisition immediately on rejection. Logs now distinguish handshake errors from handshake timeouts and record native delegate installation and challenge outcomes. Portable tests exercise accepted and rejected local TLS without contacting Reddit; native Mac acceptance is still separate.

## Difference from Muninn

| Boundary | This reader | Muninn |
| --- | --- | --- |
| Acquisition | Hidden www.reddit.com worker; HTTPS response tee and Rust post/comment parser | YouTube webviews with HTTPS response tee into structured stores |
| Proxy | Selective TLS termination for public www HTML; app-scoped CA trust and explicitly confirmed host-scoped login-Keychain fallback | Capture proxy with app-scoped trust and a keychain fallback |
| Reader | Bundled local post/comment renderer | YouTube browsing/player surfaces with cache-aware native controls |
| Media | Complete selected asset download and merge before display; range-served local files | More developed video/HLS caching and playback pipeline |
| Navigation | Subreddit listings and closeable post-reader tabs; UI preferences restored locally | Native browsing tabs, back/forward and hibernation lifecycle |
| Operations | Local cache/proxy/media diagnostics and bounded operation timings | Broader operational diagnostics, cache policies and scheduling |

The acquisition pattern and store boundaries now align; site parsers and application behavior remain independent. Muninn's broader scheduler, policy engine and browser-tab lifecycle are not implemented here.

## Native boundary

Only the bundled `main` webview receives reader/cache/fetch/diagnostics commands. The remote worker receives no application IPC capability. The app permission manifest explicitly registers local commands. See Tauri's [capabilities](https://v2.tauri.app/security/capabilities/) and [permissions](https://v2.tauri.app/security/permissions/) documentation.

Capture validates the public response path, acquisition generation, active URL, size and structured contents. Remote content cannot read cached data, invoke controller commands, read arbitrary files or run processes. SQLite writes run off the proxy I/O and WebKit navigation paths. Raw HTML is held only for bounded parsing, then discarded; cookies and authorization headers are never copied to captures or diagnostics. Private profile/login pages are not captured.

The proxy accepts HTTPS Reddit/assets destinations and rejects ordinary private/loopback addresses after DNS. Media jobs use their own downloader, not the source webview. Browser website data belongs to the app's own data store. No existing Firefox/Safari profile is imported; RES/Firefox extensions do not run inside Tauri.

## Structured content and completeness

SQLite stores thread snapshots, a reader thread/comment anchor and separate public browser URL/scroll state. The GUI and CLI share the Rust core. Imports validate parent identities, cycles, orphans and nesting, and use transactional payload eviction. Listing updates preserve previously fetched comments.

The parser handles Reddit's current markup (and classic HTML), filters promoted posts, and bounds snapshots to 512 KiB, 25 posts and 300 downloaded comments. The tee copies at most 4 MiB compressed; decompression stops above 8 MiB. Partial, failed and oversized streams never publish a capture. Opening a previously unfetched post acquires comments automatically. The downloaded HTML subset remains explicitly partial; exhaustive lazy-branch expansion is not implemented. Signed media URL query parameters are preserved.

Browser/proxy-source content expires locally after the configured post lifetime (1–8,760 hours, default 48), with a 10 MiB logical payload budget. Reads, snapshots and searches apply expiry. This does not detect remote deletions while disconnected or implement remote deletion reconciliation; extending local lifetime does not establish permission to retain remote content.

The content database has a derived FTS5 index covering post titles, bodies, subreddit names and individual comments. Transactional triggers update it alongside content replacement/deletion; existing caches are backfilled once. Search uses literal terms, a final-word prefix, title-weighted ranking and one best result per post, retaining the matching comment anchor and excerpt. Search and opening a cached result are disk-only commands; cached-only intent persists with post tabs. The header searches this cache; subreddit acquisition controls live in the subreddit panel. Post lifetime is persisted in the content database and edited in Settings, independently of watched-media retention.

## Independent completed media

Selected-post assets download into app-owned `media/<thread-id>/` directories. Images/direct MP4 use bounded HTTP downloads; supported linked video/HLS uses bundled yt-dlp and FFmpeg. HLS video/audio formats are merged into a local MP4 before display. The downloader ignores user configuration, receives no account cookies, uses argument arrays rather than a shell, and has a timeout and per-file limit. Missing fragments abort a job.

A completion manifest records source URLs and completed filenames only after the whole selected-post job succeeds. Partial jobs are not exposed to the reader. Existing completed files are reused without networking. The completed-file budget is 512 MiB, with oldest-directory eviction independent of text and a 128 MiB per-file limit. Temporary download/merge space is additional.

Watch metadata lives independently in `media/watch.sqlite`. Per-file played ranges are merged across sessions, keyed to the completed file's size/mtime fingerprint. At 95% coverage the first completion timestamp starts a 24-hour expiry, matching Muninn's default. Seek position alone grants no credit. Each player renews a separate 45-second lease every five seconds and releases it on pause/end/removal; one player cannot release another's lease. Cleanup checks startup and every minute, waits for all playable files in a post to expire, skips active leases and leaves structured content untouched. The download gate excludes cleanup during acquisition, while watch reports run independently so lease renewal is not blocked by downloads. Budget-pressure eviction also checks leases atomically; inactive media may still be removed before timed expiry when the cap requires it.

An app-owned loopback HTTP server serves only files listed in a completed job manifest. It binds a random port, requires a fresh 256-bit per-launch token, rejects path traversal, and implements HEAD and byte ranges for metadata reads and seeking. This replaced a Tauri asset-URL video path that failed in native Linux WebKit. Reader URL validation and CSP admit local media and block remote playback. Helpers are pinned and checksum-verified at build time; [license/source notices](../licenses/MEDIA-TOOLS.md) ship in the app. No runtime helper install/update is required on Mac.

## Support evidence and hot paths

Hrungnir uses the supplied artwork and Muninn's System/Classified Dark/Classified Light appearance choices. Persistent bounded JSONL logs and a native support ZIP follow Muninn's support-export approach; see [SUPPORT.md](SUPPORT.md) for contents, limits and offline export. The hidden worker has no export or logging IPC capability.

Listings commit as a batch. Routine snapshots omit full integrity scans, targeted post reads avoid full-library decoding, and completed local media bypasses the download queue. Media cache misses no longer force redundant comment acquisition. A completion notification replaces the response settling/polling delay. Client reuse validates on-disk completion so eviction cannot leave a permanent stale player URL.

## Current access evidence

The [Responsible Builder Policy](https://support.reddithelp.com/hc/en-us/articles/42728983564564-Responsible-Builder-Policy) requires explicit approval for API access. The [Data API wiki](https://support.reddithelp.com/hc/en-us/articles/16160319875092-Reddit-Data-API-Wiki) requires registered OAuth and removal of deleted content, and recommends routinely deleting stored data within 48 hours. Its rate limit is for eligible authenticated access, not anonymous website requests.

The [Data API Terms](https://redditinc.com/policies/data-api-terms) restrict retention to the approved use case and require deletion of API content on access termination. This app claims no OAuth/API entitlement. A modest subreddit JSON probe returned 403.

Reddit's [8 October changelog](https://support.reddithelp.com/hc/en-us/articles/54353370049684-Changelog-October-8-2026) says RSS support ends 13 November 2026 and provides an API migration timeline. RSS is not this app's transport foundation.

Platform limits and acceptance steps are recorded in [Mac acceptance](MACOS.md).
