# Hrungnir 0.5.3

More space for posts, clearer inline media, and a Settings pane.

- Theme, accent and media presentation move into Settings. Existing appearance preferences are retained.
- A compact toolbar and post navigation replace the permanent implementation banner and redundant reader controls.
- Posts with supported media show a type-specific inline load/show control. Text posts and ordinary article links do not show an empty media expander. Expanding downloads only that post's media before local playback.
- Media players preserve their aspect ratio within a bounded height.
- Article thumbnails and interface images are no longer treated as required post media. This fixes the `external-preview.redd.it` 403 failure seen in the supplied support ZIP, including older cached rows. Signed URLs for actual image content are retained.

The certificate recovery from 0.5.2 remains available in Diagnostics. Quit the previous version before replacing **Hrungnir.app**. Requires Apple Silicon and macOS 14+; the bundle is ad-hoc signed without notarization.

Validation: 55 Rust tests, two packaging-policy tests, Linux native build/Clippy, and native Settings/candidate-control/local-video/restart checks pass. The first post starts at y=143 at 1100×780; a local 1280×720 test video stays within 429 px without cropping or autoplay and makes no Reddit requests. The ARM app and helpers pass structural signature/resource checks. Native Mac verification of this UI revision remains pending. See [Mac instructions](MACOS.md) and [support export](SUPPORT.md).
