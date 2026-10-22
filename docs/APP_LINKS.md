# Open Reddit links from another app

Hrungnir registers the `hrungnir` URL scheme. An external app can construct:

```text
hrungnir://open?url=https%3A%2F%2Fwww.reddit.com%2Fr%2Fsoccer%2F
```

Encode the complete Reddit URL as the single `url` parameter. For example:

```js
const link = new URL('hrungnir://open');
link.searchParams.set('url', redditUrl);
```

On macOS, install Hrungnir.app in Applications, then either use the app link or send the original Reddit URL directly:

```sh
open -a Hrungnir 'https://www.reddit.com/r/soccer/'
open 'hrungnir://open?url=https%3A%2F%2Fwww.reddit.com%2Fr%2Fsoccer%2F'
```

The app starts or comes forward. A post opens a reader tab using the normal comments/media download and cache flow; an existing complete cache is reused. A subreddit opens browsing, honoring Hot/New/Top/Rising and supported Top periods. Comment permalinks request the matching downloaded comment anchor; comments absent from the downloaded HTML cannot be displayed. Links received during startup wait until reader restoration finishes. Already-running instances receive the same handoff.

Only public `/r/<community>` listing and `/r/<community>/comments/<id>/…` routes on Reddit hosts are accepted. Canonical, old, mobile and sh host aliases normalize to HTTPS www. Short redirect links such as `redd.it/...` and `/r/.../s/...` are not resolved in this version. Private/account routes, other hosts, malformed actions, credentials and nonstandard ports are rejected. Tracking parameters are discarded. Rejected links do not open a fallback browser or execute a command.

This does not make Hrungnir the system's HTTP(S) default or change another app's routing. The sender must target Hrungnir, use its app-link scheme, or use an existing URL-routing app.

Linux native tests cover executable arguments, second-instance delivery and reader routing. ARM packaging checks verify scheme registration. Actual macOS Launch Services delivery requires native Mac acceptance.
