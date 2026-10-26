# Hrungnir 0.6.11

- Choose All Reddit, the current subreddit, or Downloaded posts when searching. Downloaded search shows title, body, and comment matches with snippets and can open a matching comment without contacting Reddit.
- Match common episode formats together: `s02e08`, `2x08`, and `season 2 episode 8`. Reddit searches keep the show's surrounding phrases and include alternative episode spellings. Quoted phrases and explicit Reddit operators remain as typed; automatic matching can be disabled in Options.
- Use `j`/`k` to move focus through visible posts, subreddit names, and comments without fetching each highlighted post. Use `l` to open a post/community or expand/enter replies, and `h` to return to the list/navigator or collapse/move to a parent comment.
- Keyboard navigation respects search results and flair filters. Collapsed replies and comment focus survive redraws. Held activation keys do not queue repeated fetches, and delayed community loads do not steal focus from typing.
- `/` focuses search, Escape leaves it without discarding results, and `?` opens the shortcut guide. Typing fields, media controls, dialogs, and standard Tab navigation retain their normal behavior.
- Clear stale bulk-media progress when changing searches and debounce local suggestions while typing.

Apple Silicon, macOS 14+. The app is ad-hoc signed and not notarized. Version 0.6.10 users can check Settings for this signed update; older versions require a manual replacement.

For manual installation, download the ZIP and checksum together and verify before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.11-macos-arm64.zip.sha256
```

Validation: the full Rust suite and browser fixtures cover episode equivalence, search scope, downloaded comment matches, filtered navigation, comment trees, delayed redraws/fetches, error/retry/empty states, typing safety, and narrow layouts. Existing reader, navigation, and updater browser suites are also exercised. GitHub Actions compiles the native app and verifies the signed release artifacts. Fixture tests establish app behavior, not the completeness or relevance of Reddit's live search index.
