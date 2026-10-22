# Contributing

- Build and test as described in [README.md](README.md). Run `cargo clippy --locked --all-targets` and `cargo test --locked` before sending a change.
- Keep the bundle identifier (`io.github.iarnvidr.hrungnir`) stable; the data directory is named after it.
- Do not add features that read personal browser profiles, use account credentials, or bulk-download content.
- Tests must use synthetic fixtures (for example `example.invalid` or `127.0.0.1`), never real accounts or private hosts.
- When describing verification, distinguish mocked checks, native Linux runs and native macOS runs.
