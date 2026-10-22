# Hrungnir 0.5.2

Repairs connection recovery exposed by the supplied Mac support ZIPs.

- EOF/reset during acquisition no longer immediately destroys the worker or disables the proxy. Actual TLS alerts still fail promptly; the acquisition deadline stays bounded.
- Diagnostics offers **Allow proxy certificate…** on macOS. Explicit confirmation adds Hrungnir's own public CA to the login Keychain under an SSL policy for `www.reddit.com`. Other apps in the same user account may use this host trust. No automatic installation or system/admin trust is used. Fetch posts again after approval.
- Older broad CAs migrate to a constrained replacement once; valid replacement certificates and keys survive restarts.
- An executable `--connection-test NEW.zip` checks a listing and comments acquisition through two native workers, records actual TLS/HTTP capture, exports results and exits. See [Mac instructions](MACOS.md).

Quit the previous app before replacing **Hrungnir.app**. Requires Apple Silicon and macOS 14+. The bundle is ad-hoc signed, without Developer ID notarization.

Validation: 50 core Rust tests, two packaging-policy checks, Linux native build and Clippy, an injected-EOF acquisition regression, and a separate native connection check with 25 posts and 186 comments. The ARM cross-build and signed bundle are structurally verified. Linux checks and cross-compilation do not establish native Mac launch, Keychain approval, WKWebView acquisition or playback. Muninn's agent confirmed it also had no native Mac runner; this release closes its missing recovery-path difference without inventing Mac verification.

If acquisition still fails, export a new support ZIP. The connection test isolates content/logs, retains Hrungnir's own CA/webview store, and does not download media or install certificates.
