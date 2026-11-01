# Hrungnir contributor instructions

- Do not install or execute Node, npm, npx, or a Node-based browser driver for development, syntax checks, tests, packaging, or signing. Do not run version/availability probes for these tools. The application uses Rust and plain browser JavaScript.
- ChromeDriver is forbidden: do not download, install, probe, or launch it. Do not change Gatekeeper, quarantine, or other host security settings to run tools.
- All Rust compilation, build/test builds, and compile-based validation run in GitHub Actions. Do not run local `cargo` or Tauri build/test commands. Use the Python/CDP reader checks in the Linux Actions container; do not launch or install a host test browser. Do not substitute `node --check`.
- Use the development app configuration and an isolated data root for native tests. Never use the installed application's profile, database, cache, or settings as test data.
- Automated native journeys use `src-tauri/tauri.test.conf.json` as well as a fresh explicit data root. Its separate test identity keeps main-window preferences and local storage out of normal development sessions too. Never launch a base-configuration debug binary for these checks.
- Native reader verification uses `sh scripts/test-native-reader.sh RUN_ID` to download and verify the dedicated hidden test artifact built by that GitHub Actions run; it must not compile locally. The runner uses `src-tauri/tauri.test.conf.json` and a fresh explicit data root. Local native launches, including app-backed CLI commands, remain limited to one every 30 minutes through `scripts/native-test-window.py`. Do not launch local native apps independently from multiple agents.
- Preserve complete cached discussions when refreshing listing summaries. Source comment totals and downloaded comment counts are separate facts. A streaming video is not an offline-ready download.
- Keep paging bounded, scope-bound, ordered, and deduplicated. Loading another page must preserve existing reader state, scroll, focus, and playing media elements.
- Keep the existing meaningful assertions when changing UI controls. Validate supported layouts, enlarged text, light/dark themes, keyboard access, and reduced motion.

Run core/Tauri tests, Clippy, compilation, packaging, and the Python/CDP reader suite through GitHub Actions. The reader suite runs as `python3 tests/reader-ux/run.py` inside its Linux container. Local Python/shell policy checks and existing portable CLI commands are allowed when they do not compile or launch the app.
