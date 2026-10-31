# Hrungnir contributor instructions

- Do not install or execute Node, npm, npx, or a Node-based browser driver for development, syntax checks, tests, packaging, or signing. Do not run version/availability probes for these tools. The application uses Rust and plain browser JavaScript.
- ChromeDriver is forbidden: do not download, install, probe, or launch it. Do not change Gatekeeper, quarantine, or other host security settings to run tools.
- Run reader interaction checks through the Python/CDP runner in the Linux CI container. Do not launch or install a host test browser. JavaScript syntax and runtime checks can run in that browser or the native development app; do not substitute `node --check`.
- Use the development app configuration and an isolated data root for native tests. Never use the installed application's profile, database, cache, or settings as test data.
- Automated native journeys use `src-tauri/tauri.test.conf.json` as well as a fresh explicit data root. Its separate test identity keeps main-window preferences and local storage out of normal development sessions too. Never launch a base-configuration debug binary for these checks.
- Preserve complete cached discussions when refreshing listing summaries. Source comment totals and downloaded comment counts are separate facts. A streaming video is not an offline-ready download.
- Keep paging bounded, scope-bound, ordered, and deduplicated. Loading another page must preserve existing reader state, scroll, focus, and playing media elements.
- Keep the existing meaningful assertions when changing UI controls. Validate supported layouts, enlarged text, light/dark themes, keyboard access, and reduced motion.

Normal core checks use `cargo test --locked` and the repository's Python policy checks. Native checks also use `cargo test --locked --manifest-path src-tauri/Cargo.toml`. The existing Actions reader job runs `python3 tests/reader-ux/run.py` inside its container.
