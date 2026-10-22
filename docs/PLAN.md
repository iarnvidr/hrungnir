# Next steps

## Current slice

One classic-style reader, Muninn-style HTTPS response capture, a Rust HTML parser, automatically fetched selected-post comments, and completed local media playback. No visible remote website, DOM export or Live/Saved split. Structured content, browsing position and media retain separate storage boundaries. Subreddit/post navigation, post tabs, sorting, inline media controls and local diagnostics are implemented.

## Native Mac acceptance

Run the image/video/offline acceptance sequence in [MACOS.md](MACOS.md), including app-scoped proxy certificate trust, repeated requests that bypass the worker HTTP cache, audio, seeking, resize, failure/retry and restart. Cross-build success and native Linux results do not substitute for this.

## Comment fidelity

Expand lazy branches safely, merge newly fetched branches without discarding useful older content, handle deleted/removed markers, and improve source-layout fixtures. Success: a selected thread reports exactly what was downloaded and what remains unavailable; no complete-thread claim from a rendered subset.

## Media and policies

The selected-post media cache now exists. Next add configurable defaults/group/subreddit rules, quality/size limits, linked-file jobs, cancellation of obsolete selections, finer download progress and CLI media inspection/cleanup. Keep queues and eviction independent of text. Broaden supported hosts only with explicit tests and useful errors.

## Reader and lifecycle

Add history/back/forward and restored scroll; improve accessibility and keyboard behavior. Evaluate safer acquisition without a renderer when a supported access path exists. Account/API development still needs an approved path and native secure storage; no browser-profile import or access-denial workaround.

## Distribution

Complete native Apple Silicon verification, helper/license inventory and Developer ID/notarization when signing credentials are available. Preserve the distinction between observed app behavior and platform verification.
