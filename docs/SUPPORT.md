# Support ZIP and performance evidence

Reproduce the slowdown, open Diagnostics, and choose **Export support ZIP**. A native save dialog writes one ZIP; attach it with the action that felt slow and approximate time. Export works while a download is pending. Cancel creates no file. Choose a new filename if a ZIP already exists.

The ZIP includes:

- `report.json`: build/platform, source/proxy/TLS counters, active operations, cache usage and completed/partial media file summaries.
- `logs.json`: up to 2,000 recent structured events across the current and previous log file. Session IDs, sequence numbers, wall-clock and monotonic times distinguish restarts and event ordering.
- `timings.json`: per-stage sample counts, median and maximum durations.
- `README.txt` and `AGENTS.md`: instructions for reviewing the evidence as data.

Stages include source/media queue time, native worker startup, preference readiness, upstream headers, response body, HTML parsing, SQLite commit, worker teardown and total post/listing time. Media events record host, HTTP status, bytes, downloader duration/exit and scrubbed stderr. Local range responses and client metadata/playing/waiting/stall/error/seek events help distinguish downloading from playback and rendering.

Logs are written in the existing app data directory's `logs/` folder as `hrungnir.jsonl` and `hrungnir.previous.jsonl` (2 MiB rotation per file; a record is capped at 8 KiB). Logging writes do not perform per-event fsync. Known credential fields, URL query strings, loopback playback tokens and app/home paths are scrubbed before storage and again on export. IDs and error details remain useful evidence; inspect before sharing. Export never copies raw content databases, post/comment bodies, browser storage, CA keys or media bodies, and never uploads automatically.

After a crash or app exit, the Mac executable can export persisted evidence without acquisition:

```sh
'/Applications/Hrungnir.app/Contents/MacOS/offline-reader-desktop' --support-export "$HOME/Desktop/Hrungnir-support.zip"
```

Use the actual app location. The core developer CLI also supports `reddit-offline /path/to/cache.sqlite diagnostics export NEW.zip`. An offline export has no live counters, but retains persisted events and disk summaries.

For a bounded native acquisition check, quit the app and run the executable with `--connection-test NEW.zip` instead. It exercises a listing and comments page with separate workers and records actual TLS/HTTP capture, then exports its isolated report and exits with success/failure status. It uses Hrungnir's own CA and webview store, but a separate content/log directory. It never installs a certificate. See [MACOS.md](MACOS.md) for the command and optional, explicitly confirmed certificate recovery.

## Performance changes in 0.5

Listing imports use one transaction instead of one commit per post. Normal snapshots use cheap counters instead of an integrity scan; the CLI `doctor` still runs the explicit scan. Selected-post reads avoid decoding the whole library. Ready comments do not refetch Reddit merely because media is missing. Completed media bypasses the global download queue. In-memory post reuse checks that disk assets still exist, and concurrent opens of one post share a request. A completed proxy response wakes acquisition immediately; there is no artificial 300 ms settling delay or 100 ms polling interval.

These remove known unnecessary work. They do not establish native Mac speed; the ZIP should show whether remaining time is native startup, source access, parsing/storage, download/merge or local playback.

## Video download and retention

From 0.6.2, unwatched downloaded media also expires: the global post/unwatched-media lifetime defaults to 48 hours after download completion, with subreddit-group overrides. Reopening, Fetch all media and generating a preview do not reset that age. Once a video reaches 95% actual coverage, its separate watched-media lifetime applies. Keep video protects its post/files forever and survives restart; protection can be removed in Library. Preview failures appear as `media.poster_failed`/`media.poster_error` in support logs.

Recognized video pages and manifests use the bundled yt-dlp; direct image/MP4 URLs use HTTP. Streamin page links are supported from 0.5.4. yt-dlp downloads HLS/DASH with four concurrent fragments and one fragment retry. A complete local file is still required before display; large single-file clips therefore wait for their full transfer. Downloads of different posts are serialized, and post opening may first acquire comments.

From 0.5.4, 95% played coverage marks a video fully watched, matching Muninn's default; media expires 24 hours after that first completion. Actual played ranges are merged across sessions, so seeking near the end does not mark the skipped portion watched. Progress persists separately from post/comment content. Cleanup runs on startup and every minute while the app is open; expired files are removed on a later launch if the app was closed at expiry. Posts with multiple playable files wait for all their retention periods. Existing cached files start accumulating coverage after the update. Active players renew leases so cleanup and budget eviction skip them.

Evictable media has a 512 MiB budget and 128 MiB per-file limit. New downloads reserve 128 MiB per source; a four-source post may reserve the entire budget. Budget pressure can evict inactive unprotected media before timed expiry. Post/comment expiry uses **Settings → Post lifetime** (default 48 hours) unless a subreddit group overrides it. Groups independently configure post and watched-media lifetime, including forever. Saved Keep forever items are outside eviction budgets and survive cache clearing; counters distinguish protected and evictable text bytes. The watched threshold stays fixed at 95%; the default delay is 24 hours. **Clear cached posts** prunes unprotected structured posts only, not media. See [Library](LIBRARY.md).

The top bar searches cached titles, post bodies, subreddit names and comments. It supports multiple words and a prefix on the last word, with ranked excerpts and comment links. Searching and opening its results do not fetch Reddit or missing media, including when reopening those tabs after a restart. A listing-only cached result can therefore have no downloaded comments. Fetching a subreddit remains an explicit action in the subreddit panel.
