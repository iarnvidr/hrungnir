# Hrungnir reader design

## Visual direction

Hrungnir uses a compact, content-first reader layout. Theme, accent and media presentation belong in a Settings pane; the main toolbar is for browsing, Settings and Diagnostics. The supplied Hrungnir mark is displayed beside the name. The image is decorative because the adjacent wordmark names the app. Persistent implementation banners and redundant reader toolbars are removed to leave more space for posts and comments.

The three appearance modes follow Muninn's theme model:

- **System** follows the operating system light/dark preference and uses the system sans-serif and monospace fonts.
- **Classified dark** uses Muninn's dark charcoal surfaces and contrasting text.
- **Classified light** uses the warm parchment surfaces from the same theme family.

Blue, red, amber, and teal are selectable semantic accents. Surface, text, border, status, and focus colors come from CSS variables, so theme and accent selection apply across the reader, diagnostics dialog, media cards, and controls. Theme and accent values persist with the existing local UI preferences in `localStorage`; they do not affect cached content.

Listings receive URL-free media hints from the same Rust candidate selection used by downloads. Image, video and audio controls appear only where candidates or completed local assets exist. A candidate is not proof of a completed download. Expanding one post downloads that post through the native path before displaying a local player; media is never streamed from a remote page or automatically downloaded for the whole listing. Players preserve their aspect ratio and are height-bounded. Article thumbnails and interface images are not required media.

Classified modes use locally bundled Inter for body text, Cinzel for headings, and IBM Plex Mono for compact labels/diagnostics. These fonts and their SIL Open Font License texts are copied into `reader/fonts/` from Muninn's bundled UI resources. `font-display: swap` avoids blocking the interface. System mode has no dependency on those font files.

The UI/UX Pro Max search for a compact desktop reading tool returned the **Minimalism & Swiss Style** recommendation: grid alignment, essential controls, visible focus, high contrast, and restrained motion. Its landing-page pattern was not used; the existing application layout remains. Reduced-motion preferences are honored, links and settings retain keyboard focus rings, and error/success feedback is textual as well as colored.

## Diagnostics and performance

The Diagnostics dialog keeps copyable JSON and an optional JSON-file export. **Export support ZIP** is the primary export and invokes `export_support_bundle`; the native command owns the save dialog. The UI reports cancellation, failure, or the returned path, byte count, and event count. It does not build a Blob for the ZIP.

Long native acquisition operations show elapsed time and poll `app_diagnostics()` for `report.active_operations` stage and elapsed time. The compact recent-performance list displays only bounded operation names, status, duration, and item count from `report.operations`.

The UI sends `record_client_event({event})` with allowlisted names (`source_fetch`, `post_fetch`, `inline_media`, `reader_render`, and selected playback lifecycle events), bounded durations/counts, and small numeric media state fields. Playback samples `loadedmetadata`, `playing`, `waiting`, `stalled`, `error`, and `seeked`; it does not observe `timeupdate`. Sampling is rate-limited per player/event and to 30 events per minute. It does not send URLs, post/comment text, subreddit names, exception strings, or account data. Theme/accent are included as small preference labels for appearance diagnostics.

## Repeated post actions

Once a `ThreadDownload` has completed, `loadedThreadIds` and `mediaByThread` identify a possible in-memory hit. Before reusing it, the reader awaits `validate_cached_media({id})` so an asset evicted from disk cannot be served from stale memory; validation and any resulting refetch share one per-ID in-flight promise. A cache miss still awaits comments and media before display, and failed requests clear the in-flight entry so Retry can try again. Playback errors clear the in-memory ready marker and show an explicit retry control without starting an automatic download loop. A selected-thread snapshot merges by ID into the existing snapshot and updates the existing per-subreddit listing rather than replacing it.

## Verification scope

The root/native integration owns `export_support_bundle`, `record_client_event`, and the `app_diagnostics().report` shape. Browser checks without Tauri can verify appearance, persistence, layout, and disabled native-operation fallback only. Native export, event persistence, and live stage feedback require the integrated native commands.
