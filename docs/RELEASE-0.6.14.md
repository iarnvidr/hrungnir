# Hrungnir 0.6.14

- Fix subreddit listing assembly for aggregate feeds while keeping a selected community's results scoped to that community.
- Follow Reddit's validated `/r/popular/hot` continuation so the popular feed includes up to 25 unique posts instead of only the three initially rendered by the page.
- Fix global Reddit search parsing for live preview cards that were previously omitted; deduplicate repeated post links.
- Add live `fetch-reddit` and `search-reddit` CLI commands for native acquisition diagnostics. Existing local search remains offline.
- Use separate WebKit stores for explicit live CLI data roots so diagnostics do not reuse the installed app's browsing profile.

Known limits: Reddit redirects `/r/all` away from its public listing path, so Hrungnir reports that path as unsupported instead of displaying unrelated content. Popular results remain a page of at most 25 posts; this does not exhaust the entire feed.

Apple Silicon, macOS 14+. This app is ad-hoc signed and not notarized. Gatekeeper
may require you to approve the downloaded app in Privacy & Security. Do not
disable Gatekeeper globally. Verify the archive before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.14-macos-arm64.zip.sha256
```
