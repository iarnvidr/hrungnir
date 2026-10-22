# Native support bundle verification

For repeat-download reports on 0.6.6+, inspect `post.cache_validation` and
`post.cache` for separate comments/media hits and a `media_reason` such as
`manifest_missing`, `source_changed` or `files_missing`. `post.cache_result`
reports actual reuse after queued requests. `media.budget_eviction` identifies
the evicted and incoming posts, actual bytes and the cache limit;
`media.retention_removed` identifies expiry. These events are included in the
existing bounded support logs. `scripts/cache-reuse-smoke.py` checks repeated
native opens and restart against unchanged local video/manifest bytes, including
HLS-to-DASH identity changes, with zero Reddit requests.

This smoke check exercises the Linux Tauri app's support path with an isolated
cache. It imports only the synthetic fixture, verifies cache-hit timings and
appearance persistence, then uses the native save dialog to test ZIP export and
cancel behavior. It does not contact Reddit, fetch media, or verify macOS UI.

Run it only after the native app build, Xvfb/display, and `tauri-driver` are ready.
The XDG path must be set before `tauri-driver` starts so both it and the app use
the same isolated data directory. Start the driver in one terminal in the
container and the smoke script in another:

```sh
cd /work
mkdir -p /work/var/support-smoke/xdg
XDG_DATA_HOME=/work/var/support-smoke/xdg tauri-driver --port 4444
```

```sh
cd /work
XDG_DATA_HOME=/work/var/support-smoke/xdg \
  WEBDRIVER_URL=http://127.0.0.1:4444 \
  python3 scripts/support-smoke.py
```

The script requires an empty `/work/var/support-smoke` output directory. It
leaves `export.zip` and `support-export.png` there. Remove those two test outputs
before a later run. Do not remove the XDG directory while an app or driver is
running. The script checks that XDG data and the ZIP destination stay below the
repository's `var/support-smoke` directory.

The checks cover:

- Importing `t3_fixture`, then invoking `fetch_thread` three times. The first
  call completes an empty media manifest; the next two must report comment and
  media cache hits, finish under one second, and leave the proxy request count
  unchanged. The timing is measured with `performance.now()` around the native
  invoke inside the app page, excluding the WebDriver polling interval.
- Switching through `system`, `classified`, and `classified_light` with each of
  the `blue`, `red`, `amber`, and `teal` accents, then restarting the app and
  checking that the final `classified_light`/`teal` selection persisted.
- Canceling the native support ZIP dialog and confirming the cancellation
  status appears without creating an archive.
- Saving `/work/var/support-smoke/export` (the GTK ZIP filter appends `.zip`),
  checking the resulting `export.zip` CRC and exact five
  members (`report.json`, `logs.json`, `timings.json`, `README.txt`,
  `AGENTS.md`), validating schema and cache-hit evidence, and ensuring fixture
  post/comment text is absent. It also checks that the UI status reports the
  saved path.

The support ZIP has an intentional timestamped `Hrungnir-support-*.zip` default
name in the dialog. The smoke test replaces that name with the `export` stem to
use a stable isolated destination; the GTK ZIP filter appends the extension.
`support-export.png` captures the native app after the export. Review the ZIP
contents before sharing it.

Passing this check is Linux native UI evidence only. The save-dialog interaction
and app launch still require separate verification on macOS.

## Search and linked-content acceptance (0.6.4)

For 0.6.5 the same search field first shows local cached matches while typing;
Enter submits that query to live Reddit. `links-search-smoke.py` checks a cached
match before submission, unchanged proxy counters while typing, and a hidden
local dropdown after live results. `search-smoke.py` opens cached comment/post
matches without submitting and verifies zero Reddit requests. The deferred
`search-ui-smoke.py` covers late local responses after Enter, changing the query
during a live request, and cached-result activation staying offline.

With the same isolated native-driver setup, `python3 scripts/links-search-smoke.py`
checks local thumbnail decoding, selected public article extraction, a subsequent
disk-cache hit without an automatic Library save, and a real Reddit query plus
sort change through the response proxy. It requests only the selected article
and search pages; typing alone must not acquire source content.

Run `/usr/bin/python3 scripts/web-link-smoke.py` in a separate fresh driver
session. It requires GTK's Python Gdk bindings and xdotool, checks creation of a
distinct native window while the reader remains local, and closes it using the
native accelerator. Review `var/web-link-smoke/browser.png` for page rendering.
WebKitGTK WebDriver exposes only the original automation context, so this does
not assert a command invocation from the remote incognito page. Reader command
isolation is separately reviewed against the main-only Tauri capability.

A combined link-window/source run lost its WebDriver session after source
acquisition while the actual app still accepted native Ctrl+T and rendered a
new tab. Keep these harnesses separate; do not count that combined run as a pass
or treat the driver's generic “page crash or hang” message as proof of an app
renderer crash. These are Linux checks, separate from macOS runtime acceptance.
