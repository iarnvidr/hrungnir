# App updates

Version 0.6.10 introduces the in-app updater. Users on older versions must download and replace the app once. Subsequent releases are checked at startup and through Settings → Check for updates or Hrungnir → Check for Updates…. Installation and restart each require an explicit user action. Offline and installation failures are shown in Settings and can be retried.

The updater uses the official Tauri plugin, a public HTTPS manifest at `https://github.com/iarnvidr/hrungnir/releases/latest/download/latest.json`, and an embedded verification key. It rejects unsigned updates and signatures whose signed version does not match the manifest. Only the local bundled reader has permission to check, install, or restart; remote webviews do not.

The existing tag-triggered GitHub Actions release workflow packages the same ad-hoc signed app used for the normal ZIP into a `.app.tar.gz`, signs it with the repository secret `TAURI_SIGNING_PRIVATE_KEY`, verifies the result with minisign, and publishes the bundle, signature, and manifest. The app version is bound into the signature. The manifest is created by `scripts/package-updater.py`; this custom packaging preserves the bundled media helpers and licenses.

The signing key must remain stable across releases. A private recovery copy is kept outside the repository in the owner's Codex secrets directory; it is never included in source, artifacts, or logs. Back it up securely. Losing the key requires users to install a replacement app manually before they can receive updates signed with a new key.
