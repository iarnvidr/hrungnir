Subreddit and post acquisition now uses Muninn's HTTPS response-capture pattern: a hidden native worker requests classic Reddit, the local proxy tees complete HTML responses, and Rust parses posts/comments into SQLite. DOM export and remote capture IPC are removed. The public redesign preference is set before navigation; repeated acquisitions bypass the worker's HTTP cache.

The reader adds closeable post tabs, subreddit/post views, Hot/New/Top/Rising sorting, inline local media and full diagnostics with refresh/copy/export. Comments and supported media download before a selected post is displayed. Downloaded comment trees can still be partial.

Verification: 35 network-independent Rust tests; native Linux acquisition of public soccer listings, image/video posts and comments; disconnected restart, local playback and seeking. Apple Silicon cross-build, ad-hoc signing and bundle/resource checks pass.

Download **Offline-Reader-0.4.0-macos-arm64.zip** and compare its SHA-256 with the adjacent checksum file. Requires Apple Silicon and macOS 14+. Media helpers are bundled.

Native macOS launch, app-scoped proxied TLS trust, audio and Gatekeeper acceptance remain unverified. This build is ad-hoc signed, without Developer ID or notarization. No system/keychain trust is installed. See the application's docs/MACOS.md for acceptance steps.
