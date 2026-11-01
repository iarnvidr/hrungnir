# Hrungnir 0.6.16

- Continue global and community Reddit searches as you scroll, with a manual load-more control and retry that keeps the current results available.
- Keep existing search rows, scroll position, keyboard focus, and media elements while appending unique results. Bind continuation handles to the query and search options.
- Keep downloaded search local and preserve cached discussions when adding search result summaries.
- Load more top-level comments and replies while keeping downloaded discussions and the active player available. Save new replies to the local cache and keep Reddit's reported total separate from the number downloaded.
- Offer a fresh search or discussion refresh when a continuation expires. Remove inactive reply buttons from comments whose reply state is unknown.

Native checks use a dedicated test app identity and isolated data, separate from both the installed app and normal development preferences.

Apple Silicon, macOS 14+. The app remains ad-hoc signed and not notarized; packaging is unchanged.

Verify the downloaded archive before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.16-macos-arm64.zip.sha256
```
