Addresses the macOS TLS failure seen in the 0.5.0 support export: two rejected local TLS handshakes, zero HTTP requests/captures, followed by a 25-second timeout.

The source webview now refreshes WebKit's cached navigation-delegate capabilities after installing certificate handling. The app also declares the exact-host ATS exception needed for custom certificate evaluation on `www.reddit.com`. Navigation remains HTTPS-only, and certificate evaluation remains restricted to this installation's CA and that exact host. No keychain/system trust or global TLS bypass is installed.

TLS errors now retain their actual cause, notify the active acquisition immediately, and appear in support logs. Mac delegate installation and certificate-validation outcomes are logged too, so another failure should produce useful evidence instead of a generic timeout.

Includes the newly supplied Hrungnir logo and regenerated icons. Existing cache and settings are preserved.

All 43 Rust tests and two packaging-policy tests pass, covering TLS rejection/success, error generation tags and exact-host policy scope. A fresh native Linux run acquired 25 soccer posts through the updated proxy. The Apple Silicon cross-build and signed bundle checks pass. Native macOS WebKit acceptance remains pending; cross-compilation cannot establish that result.

Download **Hrungnir-0.5.1-macos-arm64.zip**. Quit the previous app before replacing it. If acquisition still fails, export a fresh support ZIP from this version.
