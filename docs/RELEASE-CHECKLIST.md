# Public release acceptance

Run from the reviewed PR checkout on an Apple Silicon Mac with macOS 14+.
Record the commit, macOS version, commands and results below. A checked box needs
evidence from that revision; earlier builds do not count. Do not merge, tag or
publish as part of these tests. Keep GitHub Actions disabled while the repository
is private. The owner authorized public visibility and hosted Actions on
2026-10-23. Publication of a release and the final commit history require owner approval.

## 1. Native prerequisites and portable checks

Install Xcode Command Line Tools, Homebrew and rustup first, if absent. Use a
native arm64 terminal. Python 3.12+ is required for safe tar extraction.

```sh
git rev-parse HEAD
sw_vers
uname -m                              # arm64
xcrun --find clang                    # an existing Xcode/CLT clang path
brew install node@22 python meson ninja pkgconf
export PATH="$(brew --prefix node@22)/bin:$PATH"
python3 --version                     # 3.12 or newer
node --version                        # v22.x
rustup toolchain install stable --profile minimal --component clippy --target aarch64-apple-darwin
rustup override set stable
rustc --version                       # 1.90 or newer
export TMPDIR="$(python3 -c 'import pathlib, tempfile; print(pathlib.Path(tempfile.gettempdir()).resolve())')"
npm ci
cargo test --locked
cargo clippy --locked --all-targets
python3 scripts/test-macos-policy.py
python3 scripts/test-ffmpeg-lgpl.py
node --check reader/app.js
```

- [ ] All commands exit 0. Core tests: 153 passed in total, one ignored network
  test; no failures. Policy tests: 2 pass. Recipe tests: 10 total, with host-specific
  skips on Mac. JavaScript check prints nothing on success. Review any clippy
  warnings; a missing clippy component is not a successful lint.

## 2. Build and exercise the bundled helpers

Unset any FFmpeg override so this test exercises the pinned source build. The
first run downloads yt-dlp, FFmpeg and dav1d and compiles the latter two. Subsequent
runs reuse a verified build only when its source and recipe stamp match.

```sh
unset FFMPEG_LGPL_BINARY
python3 scripts/fetch-media-tools.py
python3 scripts/ffmpeg_lgpl.py verify var/media-tools/ffmpeg
otool -L var/media-tools/ffmpeg
python3 scripts/test-media-tools.py
```

- [ ] Fetch reports `yt-dlp: publisher artifact checksum verified` and
  `ffmpeg 9.0.2: LGPL-only build verified`. Any checksum mismatch is a failure.
- [ ] Verification exits 0 and prints configuration containing `--disable-gpl`,
  `--disable-nonfree`, `--disable-autodetect`, `--enable-libdav1d`,
  `--enable-videotoolbox` and `--enable-securetransport`. No GPL, nonfree or
  version3 enable flags. Missing decoder/encoder/HTTPS support must fail.
- [ ] `otool -L` lists only `/System/Library/` and `/usr/lib/` dependencies,
  with no Homebrew or temporary dav1d library path.
- [ ] The offline generated-media test ends with
  `PASS: H.264/AAC encode, yt-dlp audio/video merge, merged decode, local JPEG poster`.
  It needs no remote video service or credentials and removes its temporary files.
  It does not prove AV1 decoding or live Reddit extraction; test those below.

## 3. Compile, assemble, sign and archive

```sh
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --locked --manifest-path src-tauri/Cargo.toml \
  --release --target aarch64-apple-darwin --features custom-protocol
python3 scripts/package-macos.py
app="artifacts/Hrungnir.app"
codesign --force --sign - "$app/Contents/Resources/bin/yt-dlp"
codesign --force --sign - "$app/Contents/Resources/bin/ffmpeg"
codesign --force --sign - "$app"
codesign --verify --deep --strict --verbose=2 "$app"
/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist"
ls "$app/Contents/Resources/licenses"
python3 scripts/archive-macos.py
(cd artifacts && shasum -a 256 -c Hrungnir-*.zip.sha256)
```

- [ ] Desktop compilation and lint exit 0. Signing verification reports
  `valid on disk` and `satisfies its Designated Requirement`.
- [ ] Bundle identifier is `io.github.iarnvidr.hrungnir`. Notices include
  `MEDIA-TOOLS.md`, `ffmpeg-COPYING.LGPLv2.1`, `ffmpeg-configuration.txt`,
  `dav1d-COPYING`, `yt-dlp-LICENSE` and `yt-dlp-THIRD_PARTY_LICENSES.txt`.
- [ ] Archive prints its SHA-256 and checksum verification reports `OK`.
  Extract the ZIP to a fresh directory with Finder or `ditto -x -k`, then repeat
  `codesign --verify --deep --strict --verbose=2` against that extracted app.
  Test the extracted app for the remaining steps. Do not repackage after approval
  without repeating signature and checksum verification.

## 4. Native launch and user acceptance

The release is ad-hoc signed and not notarized. Gatekeeper approval through
Privacy & Security may be necessary on a downloaded copy. Do not disable
Gatekeeper globally. See [MACOS.md](MACOS.md) for connection diagnostics.
Quit any other running copy before testing the extracted app.

```sh
open /path/to/extracted/Hrungnir.app
/path/to/extracted/Hrungnir.app/Contents/MacOS/offline-reader-desktop --cli help
```

- [ ] A single reader window opens, without a crash. CLI prints `Hrungnir --cli`
  usage and exits 0. Data appears in
  `~/Library/Application Support/io.github.iarnvidr.hrungnir/`.
- [ ] Refresh a subreddit; open an image post and a video with separate audio.
  Comments, local image, video poster, playback, seeking and audio work. Record
  the tested public post URLs and any extraction failures.
- [ ] Open an AV1 video, confirm its local poster is generated with the bundled
  dav1d decoder, and record the source codec and result. A format listing alone
  does not establish successful decoding.
- [ ] Confirm immediate cached search matches, then press Enter for a new Reddit
  search. Open linked article/image/video content and return to the thread.
- [ ] Quit, disconnect networking and reopen. Previously cached comments,
  images and videos remain usable, with playback and audio. Reconnect and refresh.
- [ ] Exercise save/library, restart persistence, collapsed replies, reading
  position, window resizing, portrait video fit, themes and retry after failure.
- [ ] Export a support ZIP through Diagnostics and confirm it is readable.
  Keep private browsing data and diagnostics out of public review evidence.
- [ ] Open a `hrungnir:` link as documented in [APP_LINKS.md](APP_LINKS.md);
  confirm it reaches this app and does not start a second instance.

## Review record

| Evidence | Result |
| --- | --- |
| Reviewed source commit | [f85a9a8](https://github.com/iarnvidr/hrungnir/commit/f85a9a8) |
| Native build environment | Hosted `macos-15`, Apple Silicon, stable Rust; versions in workflow logs |
| Helper build, merge and poster test | [Passed on macOS](https://github.com/iarnvidr/hrungnir/actions/runs/39999788668) |
| Core tests | 153 passed, 1 ignored on both macOS and Linux |
| Desktop compile and lint | macOS release compile passed; [Linux core/desktop clippy passed without warnings](https://github.com/iarnvidr/hrungnir/actions/runs/39999788655) |
| Signed ZIP / extracted signature / packaged CLI | [Passed; downloadable artifact](https://github.com/iarnvidr/hrungnir/actions/runs/39999788668/artifacts/12521792783) |
| Native online / offline / AV1 acceptance | Interactive checks remain unverified by hosted CI |
| Owner release approval | Pending |

The `Hrungnir-0.6.6-macos-arm64.zip` from the recorded run has SHA-256
`cb433396ad35c50c91f3f157a317f2e19dadbbd2496e7194b01f06d33cb7c735`.
This is the app ZIP checksum, distinct from GitHub's outer artifact ZIP digest.

Automated build and artifact validation passed on 2026-10-23. Interactive checks
in section 4 remain separate from compilation and automated media tests. The
source review found no missing application development to port as of 2026-10-22.
Hosted workflows are enabled with owner authorization; no self-hosted runners
are used. The macOS workflow builds, tests, signs and archives the app. Interactive
acceptance remains a separate check from compilation and automated media tests.
