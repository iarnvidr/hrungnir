# Reader UX browser checks

The five `*-check.js` files are executed as browser JavaScript against the real
reader page with deterministic native API fixtures. The Python runner drives
headless Chromium through its DevTools Protocol (CDP); it does not use Node,
Playwright, Selenium, or a browser driver.

Run only inside the Linux CI browser container. The container must provide
Python 3, Chromium, and network access to localhost. The runner creates a
temporary Chromium profile and starts the fixture server on an ephemeral local
port.

```sh
python3 -m pip install -r tests/reader-ux/requirements.txt
READER_UX_CONTAINER=1 CHROMIUM_BIN=/usr/bin/chromium python3 tests/reader-ux/run.py
```

CI can set `CHROMIUM_BIN` to the installed Chromium executable. The only Python
package is pinned in `requirements.txt`.

## Controller frame

The runner keeps check code alive in a controller page and places the reader
fixture in a same-origin, full-viewport iframe. Reload and navigation target
that iframe and wait for its load event. This preserves the existing rendered
UI assertions and scroll/viewport behavior; the suite does not assert a
top-level window navigation contract.

## Preserved coverage

| Check | Existing assertion calls | Main coverage |
| --- | ---: | --- |
| `check.js` | 24 | Listing filters, thumbnails, background tabs, immediate post/article reading, save behavior, stalled-media comments, late focus, narrow layout |
| `navigation-check.js` | 14 | Sticky navigator, independent subreddit scroll positions, library/post round trips, keyboard tab navigation, narrow layout |
| `settings-update-check.js` | 14 | Update check/failure/retry, release notes, progress, signature failure, explicit install and restart |
| `search-keyboard-check.js` | 37 | Post/comment/community hjkl, collapsed replies and redraw, search races/scopes, offline results, error/retry, narrow layout |
| `search-layout-check.js` | 19 | Search focus and arrows, community refinements, tab shortcuts, four combined layouts, overflow and persisted orientations |

All 108 existing assertion call sites and 49 `waitForFunction` waits remain in
the original check files. The facade implements only the methods those files
use. Pointer and keyboard input go through CDP input events; viewport changes,
clipboard grants, navigation, and screenshots use CDP directly. Form fill and
select/check changes use DOM value changes with standard input/change events.
