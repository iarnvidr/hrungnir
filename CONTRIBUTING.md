# Contributing

- Build and test as described in [README.md](README.md). Run `cargo clippy --locked --all-targets` and `cargo test --locked` before sending a change.
- Do not change the bundle identifier (`io.github.iarnvidr.hrungnir`) or data directory layout without a startup migration, so existing caches keep working. The one-time move from the previous identifier lives in `src/identifier_migration.rs`.
- Do not add features that read personal browser profiles, use account credentials, or bulk-download content.
- Tests must use synthetic fixtures (for example `example.invalid` or `127.0.0.1`), never real accounts or private hosts.
- When describing verification, distinguish mocked checks, native Linux runs and native macOS runs.
