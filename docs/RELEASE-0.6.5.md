# Hrungnir 0.6.5

Search now works in one field: cached post/comment matches appear immediately while typing; press Enter to search Reddit live with the same query. Selecting a cached match stays offline. The scope selector is removed, and late local responses cannot cover live results.

The existing live sorts, time filters, cached comment anchors and cached-only tab restoration remain available. Cached-only opens also avoid fetching missing remote thumbnails.

Nine deferred frontend checks and native Linux cache/live acceptance pass. The native check observed a cached suggestion in 9 ms with no source request, then 22 live results and a live sort change. Native macOS runtime verification remains pending; Apple Silicon packaging is cross-built and structurally checked from Linux.
