# Hrungnir Linux checks

This Dagger 0.21.11 module exposes `build-lint-test` and `reader-ux` entrypoints. It uses versioned Rust, Node, Python, and Playwright images and preserves the existing check commands.
Run them locally with `dagger call -m ci/dagger/check build-lint-test --source .` and `dagger call -m ci/dagger/check reader-ux --source .`.
