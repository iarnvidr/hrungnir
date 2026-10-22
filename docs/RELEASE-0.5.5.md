# Hrungnir 0.5.5

Open Reddit links from other apps using the same handoff pattern as Muninn.

- Registers `hrungnir://open?url=<encoded Reddit URL>`.
- Accepts direct URLs through macOS `open -a Hrungnir 'https://www.reddit.com/r/soccer/…'`.
- Starts or focuses Hrungnir, queues links until reader startup finishes, and forwards links to an already-running app.
- Opens posts in reader tabs using normal comment/media acquisition and completed local playback. Existing complete caches are reused. Comment permalinks focus the matching downloaded comment.
- Opens subreddit links in browsing, preserving supported listing sorts and Top periods.

Only public full subreddit/post URLs are supported; redirect short links are not resolved. Invalid or unrelated links are ignored. Hrungnir does not become the system HTTP(S) default or change other apps' routing. See [app-link instructions](APP_LINKS.md).

82 Rust tests, two packaging-policy tests, native Linux build/Clippy and ARM cross-build pass. Native executable tests cover cold arguments, comment anchors, second-instance delivery, leaving Settings, sequential queued links, invalid-link rejection and cache reuse without network. A separate public-link check acquired the selected post with 189 comments and its completed 19,207,438-byte video through yt-dlp. WebKitGTK dropped the inspector during source teardown; the still-running app's visible result was checked with a compositor screenshot rather than claiming an automated DOM/playback pass.

Quit the previous app before replacement and install Hrungnir.app in Applications. Requires Apple Silicon/macOS 14+. ARM executable/helper/signature/resource checks and packaged scheme registration pass. Native macOS Launch Services delivery remains pending.
