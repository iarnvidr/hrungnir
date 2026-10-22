# Hrungnir CLI

The installed app has a headless CLI using the same core, cache and library as the UI. It runs before Tauri/WebKit initialization. On macOS:

```sh
alias hrungnir='/Applications/Hrungnir.app/Contents/MacOS/offline-reader-desktop'
hrungnir --cli --help
hrungnir --cli library list
hrungnir --cli library search 'goal clip'
hrungnir --cli search 'cached comment'
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

Example group JSON:

```json
{"id":"football","name":"Football","subreddits":["soccer"],"post_cache_hours":72,"watched_media_hours":24}
```

Use JSON `null` instead of hours for forever. Each subreddit can belong to one group. The numeric range is 1–8,760 hours.

Link imports accept an array of HTTPS URL strings or `{"items":[{"url":"https://example.org/story","title":"Story"}]}`, plus Netscape bookmark HTML. The optional collection applies to the imported list. These are bookmarks, not downloaded articles; import reports `downloads_started: 0`. Existing archive imports preserve full saved text/media and organization. JSON/HTML imports are limited to 4 MiB/10,000 entries.

The default data directory is the installed app's existing identifier namespace. To work on an isolated library:

```sh
hrungnir --cli --data-dir /path/to/library library list
```

`HRUNGNIR_DATA_DIR` also selects a data directory. Quit the UI before performing CLI archive writes/imports so a separate process cannot change files during the operation. Read-only listings/search can run alongside the app. There is no CLI background Reddit crawler or account sign-in; open a Reddit URL in the native app to acquire its post/comments.

Development builds provide the same interface through `cargo run -- --cli …`. `HRUNGNIR_MEDIA_TOOLS` selects a helper directory for the standalone Rust development binary. The packaged app uses its bundled helpers. The older `reddit-offline DB import/read/list/position/prune/doctor` exploration commands remain supported.
