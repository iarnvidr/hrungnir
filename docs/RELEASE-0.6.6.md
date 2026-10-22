# Hrungnir 0.6.6

Fixes premature recent-media eviction: new files are staged successfully before eviction, and only their actual completed size is counted. Cache reuse refreshes LRU recency without resetting watched/unwatched retention. Failed and empty downloads preserve existing media.

Listing refreshes preserve complete cached post details, including discussions with zero comments. Equivalent Reddit HLS/DASH URLs reuse the same local video; missing generated posters no longer force video downloads.

The reader now reports **Opened from cache** when native post/media caches are reused. Support logs include cache miss reasons and byte-counted budget eviction events. The 512 MiB unprotected-media limit remains; real space pressure can still evict files sooner. Keep forever and active playback remain protected.

All 153 core tests, 12 frontend checks and native Linux cache-reuse/restart and playback regressions pass. The reuse check preserves actual video bytes/mtime/manifest and makes zero Reddit requests. Native macOS runtime verification remains pending; the Apple Silicon app is cross-built and structurally verified from Linux.
