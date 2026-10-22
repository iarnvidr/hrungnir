# Hrungnir 0.6.4

The header now searches Reddit directly. Enter a query and press Enter; choose Relevance, New, Top or Most comments and a time period. Cache remains an explicit offline search option, including comment matches and cached-only tab restoration.

Listing rows are more compact, with locally cached classic thumbnails and content-type cues. Non-media links offer inline readable articles and **Open link** in an isolated native webview. Inline extraction does not automatically save a Library item; use Library for permanent saves.

Native Linux acceptance covers a real proxy-backed search returning 22 posts across seven communities, changing its sort, cached-search/comment anchors and offline restoration, local thumbnails, real article extraction and disk-cache reuse. The external webview rendered its selected public page in a native desktop screenshot and closed through the native accelerator while preserving the reader; WebKitGTK WebDriver cannot inspect its incognito context. All 142 core tests pass, plus media/layout regressions and packaging checks. Apple Silicon builds are cross-compiled and structurally verified; native macOS launch and these new interactions remain unverified.
