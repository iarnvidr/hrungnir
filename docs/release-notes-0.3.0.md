# One local reader, downloaded media

Open **Offline Reader.app**, enter `/r/soccer` and fetch posts. Click a post:
comments and supported media download automatically before display. Images and
videos appear in the local classic-style reader; playback uses completed local
files. The modern Reddit surface, Live/Saved switches and “open live to capture
comments” message are removed.

The acquisition website runs invisibly. Promoted posts are excluded from captured
listings. Failed downloads preserve the previous reader content and offer Retry.
No account login, browser-profile import or certificate trust changes are needed.

**Apple Silicon, macOS 14+**. yt-dlp and FFmpeg are bundled; no Python/Homebrew
installation is needed. The app is ad-hoc signed, without Developer ID or
notarization, so macOS may require approval in Privacy & Security. Compare the
adjacent SHA-256 asset before opening.

Verified in native Linux Tauri/WebKit: real soccer listing, automatic selected-post
comments/image/video acquisition, completed local playback, and fresh-process
restore/playback/seeking with the container's external network disconnected.
The clip contains H.264 video and AAC audio. Seventeen Rust tests, lints and syntax
checks pass; ARM bundle/helper signatures and resource hashes are structurally
checked. **Native Mac launch and audible playback remain unverified.**

Comments are automatically fetched rendered subsets; exhaustive lazy-thread
expansion is unfinished. Configurable groups/subreddit policies, broad linked-file
coverage and account/API integration remain future work. YouTube support is not
verified. Text has a 10 MiB logical budget/local 48-hour expiry; completed media
has an independent 512 MiB budget and 128 MiB per-file limit, with temporary
download space additional. Remote deletion reconciliation is not implemented.
