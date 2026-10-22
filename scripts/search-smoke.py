#!/usr/bin/env python3
"""Native cached-search and persisted post-lifetime acceptance; synthetic data only."""
import importlib.util
import json
import os
import pathlib
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("support_smoke", ROOT / "scripts/support-smoke.py")
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)
DATA = pathlib.Path(os.environ["XDG_DATA_HOME"]).resolve()
assert DATA.is_relative_to(ROOT / "var"), "Use isolated app-owned test data"
OUTPUT = ROOT / "var/search-smoke"
OUTPUT.mkdir(parents=True, exist_ok=True)


def click(selector):
    smoke.execute("document.querySelector(arguments[0]).click()", [selector])


def search(query, expected_id):
    smoke.execute("const s=document.querySelector('#cache-search-input'); s.value=arguments[0]; s.dispatchEvent(new Event('input'));", [query])
    selector = f'.cache-search-hit[data-thread-id="{expected_id}"]'
    smoke.wait(lambda: smoke.execute("return !!document.querySelector(arguments[0])", [selector]), 10, "cached search hit")
    return selector


def ready():
    smoke.wait(lambda: smoke.execute("return !document.querySelector('#browse-submit').disabled"))


def main():
    results = {}
    try:
        smoke.start_app()
        ready()
        smoke.invoke("set_cache_settings", {"post_cache_hours": 48})
        template = json.loads((ROOT / "fixtures/thread.json").read_text())
        now = int(time.time())
        for thread in [
            dict(template, id="t3_searchmatch", title="Synthetic search acceptance", subreddit="search_lab", captured_at=now,
                 comments=[{"id": "t1_searchmatch", "parent_id": "t3_searchmatch", "body": "fjordtestneedle is in this cached comment"}],
                 missing=[], comments_status="complete"),
            dict(template, id="t3_searchlisting", title="listingonlyneedle", subreddit="search_lab", captured_at=now,
                 source="proxy", permalink="https://www.reddit.com/r/search_lab/comments/searchlisting/test/",
                 comments=[], missing=[], comments_status="not_loaded"),
            dict(template, id="t3_searchexpired", title="expireduniqueneedle", subreddit="search_lab", captured_at=now-10800,
                 source="proxy", comments=[], missing=[], comments_status="empty_confirmed"),
        ]:
            smoke.invoke("import_json", {"json": json.dumps(thread)})
        smoke.stop_app()
        smoke.start_app()
        ready()
        results["layout"] = smoke.execute("return {sidebarForm:!!document.querySelector('aside #browse-form'), headerSearch:!!document.querySelector('header #cache-search-input'), headerFetch:!!document.querySelector('header #browse-submit')}")
        assert results["layout"] == {"sidebarForm": True, "headerSearch": True, "headerFetch": False}, results["layout"]
        selector = search("fjordtestneedle", "t3_searchmatch")
        assert smoke.execute("return document.querySelector(arguments[0]).dataset.commentId", [selector]) == "t1_searchmatch"
        assert "fjordtestneedle" in smoke.execute("return document.querySelector(arguments[0]).textContent", [selector])
        click(selector)
        smoke.wait(lambda: smoke.execute("return !!document.querySelector('#thread-view [data-comment-id=\"t1_searchmatch\"].anchor-target')"))
        results["comment_search_and_open"] = True
        selector = search("listingonlyneedle", "t3_searchlisting")
        click(selector)
        smoke.wait(lambda: smoke.execute("return document.querySelector('#thread-view').textContent.includes('listingonlyneedle')"))
        # Both switching tabs and a cold restart must retain the cached-only intent.
        for title in ["Synthetic search acceptance", "listingonlyneedle"]:
            click(f'.post-tab-open[aria-label="Open {title} from r/search_lab"]')
            smoke.wait(lambda: smoke.execute("return document.querySelector('#thread-view').textContent.includes(arguments[0]) && !document.querySelector('#reader-feedback').textContent.includes('Opening')", [title]))
        smoke.stop_app()
        smoke.start_app()
        ready()
        smoke.wait(lambda: smoke.execute("return document.querySelector('#thread-view').textContent.includes('listingonlyneedle')"))
        results["listing_only_restore"] = True
        click("#settings-open")
        smoke.wait(lambda: smoke.execute("return document.querySelector('#post-cache-hours').value === '48'"))
        smoke.execute("const h=document.querySelector('#post-cache-hours'); h.value='1'; h.dispatchEvent(new Event('input'));")
        click("#post-cache-apply")
        smoke.wait(lambda: smoke.invoke("get_cache_settings")["post_cache_hours"] == 1)
        assert not smoke.invoke("search_cache", {"query": "expireduniqueneedle", "subreddit": None, "limit": 50})
        assert smoke.invoke("search_cache", {"query": "fjordtestneedle", "subreddit": None, "limit": 50})
        click("#settings-close")
        smoke.stop_app()
        smoke.start_app()
        ready()
        assert smoke.invoke("get_cache_settings")["post_cache_hours"] == 1
        results["lifetime_apply_prune_and_restart"] = True
        assert smoke.proxy_requests(smoke.invoke("app_diagnostics")) == 0, "Cached search/open/restore must not acquire Reddit"
        results["no_source_requests"] = True
        search("fjordtestneedle", "t3_searchmatch")
        smoke.screenshot(OUTPUT / "cached-search.png")
        results.update(status="passed", platform="linux")
        (OUTPUT / "result.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(results), flush=True)
    finally:
        smoke.stop_app()


if __name__ == "__main__":
    main()
