# Hrungnir CLI

The installed app has a CLI for local cache/library operations and live public Reddit acquisition. Existing local commands run headlessly. `fetch-reddit` and `search-reddit` start the native Tauri/WKWebView worker and return one JSON result to stdout. On macOS:

```sh
alias hrungnir='/Applications/Hrungnir.app/Contents/MacOS/offline-reader-desktop'
hrungnir --cli --help
hrungnir --cli library list
hrungnir --cli library search 'goal clip'
hrungnir --cli search 'cached comment'
hrungnir --cli fetch-reddit soccer --sort hot --period day
hrungnir --cli fetch-reddit soccer --sort hot --period day --pages 3
hrungnir --cli fetch-reddit all
hrungnir --cli search-reddit 'goal clip' --subreddit soccer
hrungnir --cli search-reddit rust --sort relevance --period all
hrungnir --cli library save t3_POST_ID Football
hrungnir --cli library save-url 'https://example.org/article' article Reading
hrungnir --cli library save-url 'https://www.youtube.com/watch?v=VIDEO_ID' video Clips
hrungnir --cli library save-url 'https://example.org/photo.jpg' image Photos
hrungnir --cli library export "$HOME/Desktop/Hrungnir-library.zip"
hrungnir --cli library import "$HOME/Desktop/Hrungnir-library.zip"
hrungnir --cli import links "$HOME/Desktop/bookmarks.html" Imported
hrungnir --cli library export-links "$HOME/Desktop/links.json"
hrungnir --cli groups list
hrungnir --cli groups put "$HOME/Desktop/football-group.json"
hrungnir --cli cache settings 72
hrungnir --cli doctor
```

Replace example URLs/IDs with the chosen item. `library save` protects an already-cached post; `save-url` downloads one explicitly selected linked article/video/image. Saves default to forever. `library remove ID` releases library membership/protection without immediately deleting files. `groups delete ID` removes the policy and membership, returning those subreddits to global defaults.

`search QUERY` and `library search QUERY` stay local. `search-reddit QUERY` acquires public Reddit search results; add `--subreddit NAME` for a community-scoped search. Live commands accept `--sort` and `--period` options and return a nonzero process status with JSON error output if acquisition fails. An empty search is a successful empty result. Reddit routes that redirect outside public community pages, such as the current `/r/all` redirect, remain unavailable; the CLI does not follow them into account or home pages.

`fetch-reddit --pages N` requests 1–20 subreddit-listing pages (default 1). The first page contains the posts Reddit server-rendered for that listing; it can have fewer than 25 posts. Each later page consumes one validated continuation fragment and returns at most 25 posts, along with an opaque cursor and explicit exhaustion state. Pagination deduplicates post IDs and keeps complete cached post bodies/comments locally; listing rows carry source-reported `comment_count` separately from the cached comment subset. A listing refresh can update that total without replacing the cached discussion. Search results do not currently paginate. Cursors are tied to the listing scope and expire after 15 minutes; refresh the listing if one expires.

Example group JSON:

```json
{"id":"football","name":"Football","subreddits":["soccer"],"post_cache_hours":72,"watched_media_hours":24}
```

Use JSON `null` instead of hours for forever. Each subreddit can belong to one group. The numeric range is 1–8,760 hours.

Link imports accept an array of HTTPS URL strings or `{"items":[{"url":"https://example.org/story","title":"Story"}]}`, plus Netscape bookmark HTML. The optional collection applies to the imported list. These are bookmarks, not downloaded articles; import reports `downloads_started: 0`. Existing archive imports preserve full saved text/media and organization. JSON/HTML imports are limited to 4 MiB/10,000 entries.

The default data directory is the installed app's existing identifier namespace. `--data-dir` and `HRUNGNIR_DATA_DIR` apply to both local and live CLI commands. To work on an isolated library:

```sh
hrungnir --cli --data-dir /path/to/library library list
```

For a live development smoke check, build or launch the separate development app with `sh scripts/dev.sh`, then use its debug binary and a new absolute data directory for each operation:

```sh
BIN=/Users/et/Documents/Codex/2026-10-31/sim/hrungnir/src-tauri/target/debug/offline-reader-desktop
"$BIN" --cli fetch-reddit soccer --data-dir /private/tmp/hrungnir-live-soccer
"$BIN" --cli fetch-reddit soccer --sort hot --period day --pages 3 --data-dir /private/tmp/hrungnir-live-soccer-pages
"$BIN" --cli search-reddit rust --data-dir /private/tmp/hrungnir-live-global-search
"$BIN" --cli search-reddit goal --subreddit soccer --data-dir /private/tmp/hrungnir-live-soccer-search
"$BIN" --cli --data-dir /private/tmp/hrungnir-local-search search 'cached comment'
```

Live commands print one JSON object. Successful acquisition, including an empty result, exits 0; invalid arguments exit 2; acquisition errors exit 1. These commands exercise native Reddit acquisition without opening the production app's data directory. The local `search` example reads only its selected cache.

Quit the UI before performing CLI archive writes/imports so a separate process cannot change files during the operation. Read-only local search can run alongside the app. Live Reddit commands use the same native public-only acquisition path as the desktop UI and do not use account credentials.

Development builds provide the same interface through `cargo run -- --cli …`. `HRUNGNIR_MEDIA_TOOLS` selects a helper directory for the standalone Rust development binary. The packaged app uses its bundled helpers. The older `reddit-offline DB import/read/list/position/prune/doctor` exploration commands remain supported.
