# Hrungnir 0.6.10

- Open Settings with Cmd+, on macOS or Ctrl+, on other desktops, including while editing a text field. The macOS application menu now includes Settings… and Check for Updates….
- Check for updates automatically at startup, or from Settings. Available versions, release notes, download progress, and retryable failures are visible in Settings.
- Install updates inside the app after signature and signed-version verification. Installation and restarting are separate actions you choose; an update never automatically restarts your session.
- Publish a signed macOS updater bundle and latest.json from GitHub Actions alongside the standard ZIP and checksum.

**Upgrading from 0.6.9 or earlier:** download the ZIP and replace Hrungnir.app once. Earlier versions have no updater and cannot discover or install this release themselves. Version 0.6.10 can receive subsequent releases through Settings.

Apple Silicon, macOS 14+. The app is ad-hoc signed and not notarized.

Download both ZIP files to the same folder and verify before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.10-macos-arm64.zip.sha256
```

Validation includes browser fixtures for Settings shortcuts and native menu events, startup/manual update checks, offline retry, progress, signature failure, and explicit installation/restart. GitHub Actions compiles on Linux and macOS. Release packaging verifies updater signatures independently with minisign and checks the extracted app and packaged CLI. Browser fixtures do not exercise installing over an app on a user's Mac.
