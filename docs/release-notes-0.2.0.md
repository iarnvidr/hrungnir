# Live Reddit browsing on Apple Silicon

Unzip `Offline-Reader-0.2.0-macos-arm64.zip`, open **Offline Reader.app**, enter `/r/soccer` and click **Browse**. **Live** is the actual Reddit website. Open a post to capture its rendered comments; **Saved** closes the live view and reads the captured text from SQLite. **Open live post** returns to the website. No fixture import is required.

This replaces the fixture-only 0.1.0 exploration. The architecture follows Muninn's boundaries: native Tauri controls/browser, a local CONNECT proxy, structured SQLite content and separate browsing state. TLS remains unchanged; rendered public DOM supplies capture. No CA/keychain changes or browser-profile import is needed.

Requires **Apple Silicon and macOS 14+**. The app is ad-hoc signed, without Developer ID/notarization; macOS may require approval in Privacy & Security. Compare the adjacent SHA-256 asset before opening.

Validation: 11 Rust integration tests, formatting/Clippy and JavaScript checks; native Linux Tauri/WebKit browsed soccer, captured four real posts and 25 comments through the actual UI, read Saved content and verified persistence/selected-thread restoration after restarting. The ARM Mach-O bundle and ad-hoc code/resource hashes are checked. **Native macOS launch and WKWebView behavior have not been tested on a Mac.**

Captured comments are partial and limited to rendered text. Media downloads/offline images, automatic lazy-comment expansion, account integration, full browser history/scroll restoration and subreddit/group policies remain unimplemented. Browser-source snapshots have a 10 MiB logical payload budget and local 48-hour expiry; this does not implement remote deletion reconciliation.
