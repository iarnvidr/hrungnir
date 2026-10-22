#!/usr/bin/env python3
"""Native Linux tab accelerators and inline batch cache acceptance.

Uses only synthetic posts and completed local video fixtures. Run with a private
tauri-driver inheriting XDG_DATA_HOME below this app's var directory.
"""
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('support_smoke', ROOT / 'scripts/support-smoke.py')
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)
DATA = pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert DATA.is_relative_to(ROOT / 'var')
APP = DATA / json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())['identifier']
OUT = ROOT / 'var/batch-media-smoke'


def click(selector):
    smoke.execute('document.querySelector(arguments[0]).click()', [selector])


def main():
    results = {}
    try:
        smoke.start_app()
        smoke.wait(lambda: smoke.execute('return !document.querySelector("#browse-submit").disabled'))
        template = json.loads((ROOT / 'fixtures/thread.json').read_text())
        for n in range(8):
            ident = f't3_batch{n}'
            selected = ROOT / 'var/streamff061/t3_1x8eukv/asset-0.mp4'
            real_clip = n == 0 and selected.is_file()
            link = 'https://streamff.pro/v/97600ad1' if real_clip else f'https://v.redd.it/batch{n}'
            thread = dict(template, id=ident, subreddit='soccer', title=f'Synthetic cached clip {n} with a long tab title',
                          captured_at=int(time.time()), comments=[], missing=[], source='proxy', comments_status='empty_confirmed',
                          link_url=link if n < 3 else None, media_urls=[])
            smoke.invoke('import_json', {'json': json.dumps(thread)})
            if n < 3:
                directory = APP / 'media' / ident
                directory.mkdir(parents=True, exist_ok=True)
                fixture = ROOT / 'var/portrait-fixture.mp4' if n == 2 and (ROOT / 'var/portrait-fixture.mp4').exists() else ROOT / 'var/compact-fixture.mp4'
                shutil.copy2(selected if real_clip else fixture, directory / 'clip.mp4')
                source = link if real_clip else f'https://v.redd.it/batch{n}/HLSPlaylist.m3u8'
                (directory / 'complete.json').write_text(json.dumps({'sources': [source], 'files': ['clip.mp4']}))
        smoke.stop_app()
        smoke.start_app()
        smoke.wait(lambda: smoke.execute('return document.querySelectorAll(".thread-item").length === 8'))
        click('#fetch-all-media')
        smoke.wait(lambda: smoke.execute('return document.querySelectorAll(".inline-media video").length === 3'))
        smoke.wait(lambda: smoke.execute('return document.querySelector("#fetch-all-media").textContent === "Fetch all media"'))
        results['batch'] = smoke.execute('return {status:document.querySelector("#fetch-all-media-status").textContent, local:[...document.querySelectorAll(".inline-media video")].every(v=>v.src.startsWith("http://127.0.0.1:") && v.controls && !v.autoplay)}')
        assert results['batch']['local']
        smoke.execute('window.__selectedVideo=document.querySelector("[data-thread-id=t3_batch0] .inline-media video")')
        results['layout'] = smoke.execute('const v=window.__selectedVideo;return {player:v.getBoundingClientRect().width,container:v.parentElement.getBoundingClientRect().width,poster:v.poster,time:v.currentTime,paused:v.paused}')
        assert results['layout']['player'] >= results['layout']['container'] - 3
        assert results['layout']['poster'].startswith('http://127.0.0.1:')
        assert results['layout']['time'] == 0 and results['layout']['paused']
        smoke.execute('window.__posterProbe=new Image();window.__posterProbe.src=window.__selectedVideo.poster')
        smoke.wait(lambda: smoke.execute('return window.__posterProbe.complete && window.__posterProbe.naturalWidth > 0'))
        results['first_frame_decodes_without_playing'] = True
        if (ROOT / 'var/portrait-fixture.mp4').exists():
            smoke.execute('window.__portraitProbe=new Image();window.__portraitProbe.src=document.querySelector("[data-thread-id=t3_batch2] video").poster')
            smoke.wait(lambda: smoke.execute('return window.__portraitProbe.complete && window.__portraitProbe.naturalHeight > window.__portraitProbe.naturalWidth'))
            portrait = smoke.execute('const v=document.querySelector("[data-thread-id=t3_batch2] video"),r=v.getBoundingClientRect();return {width:r.width,height:r.height,container:v.parentElement.getBoundingClientRect().width,ratio:window.__portraitProbe.naturalWidth/window.__portraitProbe.naturalHeight,zoom:Number(getComputedStyle(document.body).zoom)||1,viewportHeight:innerHeight}')
            assert portrait['width'] <= portrait['container'] + 1
            assert portrait['height'] * portrait['zoom'] <= portrait['viewportHeight'] - 155, portrait
            assert abs(portrait['width'] / portrait['height'] - portrait['ratio']) < .03, portrait
            results['portrait_fits_viewport_without_stretching'] = portrait
        smoke.execute('const v=window.__selectedVideo;v.muted=true;v.play().catch(e=>window.__playError=String(e))')
        time.sleep(4)
        results['selected_playback'] = smoke.execute('const v=window.__selectedVideo;return {time:v.currentTime,duration:Number.isFinite(v.duration)?v.duration:null,readyState:v.readyState,error:v.error?.message || window.__playError || null}')
        assert results['selected_playback']['error'] is None
        assert results['selected_playback']['time'] > 1
        if results['selected_playback']['duration'] > 12:
            smoke.execute('window.__selectedVideo.currentTime=10')
            smoke.wait(lambda: smoke.execute('return window.__selectedVideo.currentTime > 10.25'))
            results['selected_seek'] = True
        smoke.execute('window.__selectedVideo.pause()')
        assert smoke.proxy_requests(smoke.invoke('app_diagnostics')) == 0
        results['zero_source_requests'] = True
        # A second run must preserve existing inline player nodes.
        smoke.execute('window.__retainedPlayer=window.__selectedVideo')
        click('#fetch-all-media')
        smoke.wait(lambda: smoke.execute('return document.querySelector("#fetch-all-media").textContent === "Fetch all media"'))
        assert smoke.execute('return window.__retainedPlayer.isConnected'), 'Batch must preserve existing players'
        results['preserved_player'] = True
        click('[data-thread-id=t3_batch0] .inline-media .keep-video-button')
        smoke.wait(lambda: smoke.execute('return document.querySelector("[data-thread-id=t3_batch0] .inline-media .keep-video-button").textContent === "Kept forever"'))
        assert smoke.execute('return window.__retainedPlayer.isConnected')
        saved = smoke.invoke('library_state')['items']
        assert any(item['id'] == 't3_batch0' and item['keep_forever'] for item in saved)
        results['inline_keep_protects_post_and_player'] = True
        click('#settings-open')
        smoke.execute('for(const [id,value] of [["reader-text-scale",125],["reader-ui-scale",110]]){const e=document.getElementById(id);e.value=value;e.dispatchEvent(new Event("input",{bubbles:true}));e.dispatchEvent(new Event("change",{bubbles:true}));}')
        assert smoke.execute('return document.documentElement.style.getPropertyValue("--reader-text-scale")') == '1.25'
        assert smoke.execute('return document.documentElement.style.getPropertyValue("--reader-ui-scale")') == '1.1'
        results['independent_scale_settings'] = True
        click('#settings-close')
        if (ROOT / 'var/portrait-fixture.mp4').exists():
            zoomed = smoke.execute('const v=document.querySelector("[data-thread-id=t3_batch2] video"),r=v.getBoundingClientRect();return {height:r.height,zoom:Number(getComputedStyle(document.body).zoom)||1,viewportHeight:innerHeight}')
            assert zoomed['height'] > 0 and zoomed['height'] * zoomed['zoom'] <= zoomed['viewportHeight'] - 155
            results['portrait_height_respects_element_zoom'] = zoomed
        for n in range(8):
            click('#tab-browse')
            click(f'[data-thread-id="t3_batch{n}"] .thread-open')
            smoke.wait(lambda: smoke.execute('return document.querySelector(".post-tab-open[aria-selected=true]")?.textContent.includes(arguments[0])', [f'Synthetic cached clip {n} ']))
        results['tabs_before'] = smoke.execute('return document.querySelectorAll(".post-tab").length')
        assert results['tabs_before'] >= 8
        subprocess.run(['xdotool', 'key', '--clearmodifiers', 'ctrl+w'], check=True)
        smoke.wait(lambda: smoke.execute('return document.querySelectorAll(".post-tab").length') == results['tabs_before'] - 1)
        results['native_close_tab'] = True
        subprocess.run(['xdotool', 'key', '--clearmodifiers', 'ctrl+t'], check=True)
        smoke.wait(lambda: smoke.execute('return document.activeElement.id === "subreddit-input"'))
        results['native_new_tab'] = True
        assert smoke.execute('return document.querySelectorAll(".post-tab").length') == results['tabs_before']
        subprocess.run(['xdotool', 'key', '--clearmodifiers', 'ctrl+w'], check=True)
        smoke.wait(lambda: smoke.execute('return document.querySelectorAll(".post-tab").length') == results['tabs_before'] - 1)
        results['native_close_draft'] = True
        smoke.stop_app()
        smoke.start_app()
        smoke.wait(lambda: smoke.execute('return document.querySelector("#reader-text-scale").value === "125" && document.querySelector("#reader-ui-scale").value === "110"'))
        assert any(item['id'] == 't3_batch0' and item['keep_forever'] for item in smoke.invoke('library_state')['items'])
        results['settings_and_keep_survive_restart'] = True
        results['scale_layouts'] = []
        for width in [1100, 720]:
            smoke.request('POST', f'/session/{smoke.session}/window/rect', {'width': width, 'height': 780})
            for text, elements in [(85, 85), (140, 125)]:
                click('#settings-open')
                smoke.execute('for(const [id,value] of [["reader-text-scale",arguments[0]],["reader-ui-scale",arguments[1]]]){const e=document.getElementById(id);e.value=value;e.dispatchEvent(new Event("input",{bubbles:true}));e.dispatchEvent(new Event("change",{bubbles:true}));}', [text, elements])
                click('#settings-close')
                geometry = smoke.execute('return {width:innerWidth,documentWidth:document.documentElement.scrollWidth}')
                assert geometry['documentWidth'] <= geometry['width'] + 1, geometry
                results['scale_layouts'].append(dict(geometry, text=text, elements=elements))
        smoke.screenshot(OUT / 'tabs.png')
        results['status'] = 'passed'
        results['platform'] = 'Linux WebKitGTK; macOS accelerators cross-built only'
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / 'result.json').write_text(json.dumps(results, indent=2))
        print(json.dumps(results), flush=True)
    except Exception:
        print(json.dumps(results), flush=True)
        raise
    finally:
        smoke.stop_app()


if __name__ == '__main__':
    OUT.mkdir(parents=True, exist_ok=True)
    main()
