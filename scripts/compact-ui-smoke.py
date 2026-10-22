#!/usr/bin/env python3
"""Native Linux Settings, media-cue and cached inline playback acceptance.

Requires the isolated tauri-driver environment described by support-smoke.py,
plus var/compact-fixture.mp4 (a three-second synthetic H.264 baseline test
pattern without B-frames; the headless Linux decoder stalls on the High fixture).
No Reddit acquisition or remote media download is required.
"""
import importlib.util
import json
import os
import pathlib
import shutil
import sqlite3
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("support_smoke", ROOT / "scripts/support-smoke.py")
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)
DATA = pathlib.Path(os.environ["XDG_DATA_HOME"]).resolve()
assert DATA.is_relative_to(ROOT / "var"), "Use isolated app-owned test data"
OUTPUT = ROOT / "var/compact-ui-smoke"
OUTPUT.mkdir(parents=True, exist_ok=True)
CONFIG = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
APP_DATA = DATA / CONFIG["identifier"]


def click(selector):
    smoke.execute("document.querySelector(arguments[0]).click()", [selector])


def main():
    results = {}
    try:
        smoke.start_app()
        smoke.wait(lambda: smoke.execute("return !document.querySelector('#browse-submit').disabled"))
        template = json.loads((ROOT / "fixtures/thread.json").read_text())
        for name, link, media in [
            ("text", None, []),
            ("article", "https://example.org/story", ["https://external-preview.redd.it/thumb.jpg?s=signed"]),
            ("video", "https://v.redd.it/clip123", []),
        ]:
            thread = dict(template, id=f"t3_compact_{name}", subreddit="soccer", title=f"Synthetic {name} post",
                          comments=[], missing=[], comments_status="empty_confirmed", link_url=link, media_urls=media)
            smoke.invoke("import_json", {"json": json.dumps(thread)})
        media_root = APP_DATA / "media/t3_compact_video"
        media_root.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / "var/compact-fixture.mp4", media_root / "media.mp4")
        (media_root / "complete.json").write_text(json.dumps({
            "sources": ["https://v.redd.it/clip123/HLSPlaylist.m3u8"], "files": ["media.mp4"]}))
        smoke.stop_app()
        smoke.start_app()
        smoke.wait(lambda: smoke.execute("return document.querySelectorAll('.thread-item').length === 3"))
        results["cues"] = smoke.execute("""
          return Object.fromEntries([...document.querySelectorAll('.thread-item')].map(row =>
            [row.dataset.threadId, !!row.querySelector('.inline-media-toggle')]));
        """)
        assert results["cues"] == {"t3_compact_text": False, "t3_compact_article": False, "t3_compact_video": True}
        results["geometry"] = smoke.execute("""
          return {firstRowY: document.querySelector('.thread-item').getBoundingClientRect().top,
                  width: innerWidth, documentWidth: document.documentElement.scrollWidth,
                  banner: !!document.querySelector('.notice'), appearancePopup: !!document.querySelector('#appearance-menu')};
        """)
        assert results["geometry"]["firstRowY"] < 160, results["geometry"]
        assert results["geometry"]["documentWidth"] <= results["geometry"]["width"]
        assert not results["geometry"]["banner"] and not results["geometry"]["appearancePopup"]
        click("#settings-open")
        assert smoke.execute("return document.querySelector('#settings-dialog').open")
        smoke.set_appearance("classified", "teal")
        smoke.execute("const s=document.querySelector('#media-presentation'); s.value='collapsed'; s.dispatchEvent(new Event('change'))")
        subprocess.run(["xdotool", "key", "Escape"], check=True)
        smoke.wait(lambda: smoke.execute("return !document.querySelector('#settings-dialog').open"))
        assert smoke.execute("return document.activeElement.id") == "settings-open"
        click('[data-thread-id="t3_compact_video"] .inline-media-toggle')
        smoke.wait(lambda: smoke.execute("return !!document.querySelector('.inline-media video')"))
        results["player"] = smoke.execute("""
          const v=document.querySelector('.inline-media video');
          return {src:v.currentSrc || v.src, autoplay:v.autoplay, controls:v.controls,
                  height:v.getBoundingClientRect().height, maxHeight:innerHeight * .55};
        """)
        assert results["player"]["src"].startswith("http://127.0.0.1:")
        assert not results["player"]["autoplay"] and results["player"]["controls"]
        assert results["player"]["height"] <= results["player"]["maxHeight"] + 1
        smoke.execute("const v=document.querySelector('.inline-media video'); v.muted=true; v.play().catch(e => { window.__playError=String(e); });")
        smoke.wait(lambda: smoke.execute("return document.querySelector('.inline-media video')?.currentTime > .2"))
        results["player"]["height"] = smoke.execute("return document.querySelector('.inline-media video').getBoundingClientRect().height")
        assert results["player"]["height"] <= results["player"]["maxHeight"] + 1
        assert smoke.execute("return getComputedStyle(document.querySelector('.inline-media video')).objectFit") == "contain"
        results["local_playback"] = True
        smoke.execute("const v=document.querySelector('.inline-media video'); v.currentTime=v.duration-.04; v.play().catch(() => {});")
        smoke.wait(lambda: smoke.execute("return document.querySelector('.inline-media video')?.ended === true"), 15, "seek to video end")
        seek_state = smoke.invoke("app_diagnostics")["media"]["watch_retention"]["state"]
        assert not any(entry.get("fully_watched_at") for entry in seek_state.get("entries", [])), "Seeking to the end must not count as a full watch"
        results["seek_does_not_complete"] = True
        smoke.execute("const v=document.querySelector('.inline-media video'); v.currentTime=0; v.play().catch(() => {});")
        # Let the short clip finish before taking compositor screenshots/dialogs.
        smoke.wait(lambda: smoke.execute("return document.querySelector('.inline-media video')?.ended === true"), 15, "full local video watch")
        report = smoke.invoke("app_diagnostics")
        assert smoke.proxy_requests(report) == 0, "Synthetic cached playback must not acquire Reddit"
        smoke.screenshot(OUTPUT / "inline.png")
        click("#settings-open")
        smoke.wait(lambda: smoke.execute("return document.querySelector('#settings-dialog').open"))
        # GTK's screenshot observes the compositor, not just DOM layout state.
        time.sleep(.25)
        smoke.screenshot(OUTPUT / "settings.png")
        click("#settings-close")
        smoke.wait(lambda: smoke.execute("return document.querySelector('.inline-media video')?.ended === true"), 15, "full local video watch")
        def completed_watch():
            state = smoke.invoke("app_diagnostics")["media"]["watch_retention"]["state"]
            entries = [entry for entry in state.get("entries", []) if entry["thread_id"] == "t3_compact_video"]
            return entries[0] if entries and entries[0].get("fully_watched_at") else None
        results["watch"] = smoke.wait(completed_watch, 10, "persistent full watch report")
        assert results["watch"]["watched_fraction"] >= .95
        assert results["watch"]["expires_at"] - results["watch"]["fully_watched_at"] == 86400
        smoke.stop_app()
        smoke.start_app()
        smoke.wait(lambda: smoke.execute("return document.documentElement.dataset.accent === 'teal'"))
        results["preferences"] = smoke.execute("""
          return {theme:document.querySelector('#theme-select').value, accent:document.querySelector('#accent-select').value,
                  media:document.querySelector('#media-presentation').value};
        """)
        assert results["preferences"] == {"theme": "classified", "accent": "teal", "media": "collapsed"}
        assert completed_watch()["fully_watched_at"] == results["watch"]["fully_watched_at"]
        smoke.stop_app()
        # Advance only the isolated test record, never the system clock.
        with sqlite3.connect(APP_DATA / "media/watch.sqlite") as db:
            db.execute("UPDATE watches SET completed=? WHERE post=?", (int(time.time()) - 86401, "t3_compact_video"))
        smoke.start_app()
        smoke.wait(lambda: not media_root.exists(), 10, "expired watched media cleanup on restart")
        assert smoke.invoke("snapshot")["threads"], "Media cleanup must preserve structured posts"
        results["watched_expiry_restart_cleanup"] = True
        results["status"] = "passed"
        results["platform"] = "linux"
        (OUTPUT / "result.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(results), flush=True)
    finally:
        smoke.stop_app()


if __name__ == "__main__":
    main()
