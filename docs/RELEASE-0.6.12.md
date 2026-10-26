# Hrungnir 0.6.12

- Public search preserves the original query instead of replacing episode queries with restrictive Boolean requirements. Relevance additionally searches the series name, merges unique posts, and boosts discussion/episode matches. Advanced queries and other sorts remain under Reddit's control. Use the community buttons above results to refine a query within a community.
- Cmd+L / Ctrl+L focuses search, alongside `/`. Search submissions by keyboard or mouse focus results, including when the native webview blurs the submitting control while waiting. Moving elsewhere while waiting prevents focus theft.
- Arrow keys work alongside hjkl in search results, subreddit lists and comment trees. Shift+H/K or Shift+Left/Up switches to the previous tab; Shift+J/L or Shift+Down/Right switches to the next. On a subreddit tab these switch communities, preserving their order.
- Choose post and subreddit tab orientations independently under Settings → Tabs: both horizontal, both vertical, or either mixed arrangement. Both navigators stay visible while reading. Library moves to the toolbar. Navigation, search and Library remain available while scrolling, and each subreddit/post retains its position.

Apple Silicon, macOS 14+. Update through Settings → Check for updates, or download the ZIP and verify its checksum before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.12-macos-arm64.zip.sha256
```

Validation: Rust tests cover the native-site example titles, partial matching, advanced query preservation and local episode equivalents. Browser checks exercise keyboard/mouse result focus, tab switching, all four layouts, narrow windows, saved orientations, typing safety, reader behavior, scroll restoration and updates. GitHub Actions builds the native macOS app and verifies the signed release artifacts. A live Reddit request from the development container returned HTTP 403, so live result completeness/ranking has not been measured against the website. Community refinements come from returned posts; live community autocomplete is not included.
