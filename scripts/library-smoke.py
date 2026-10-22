#!/usr/bin/env python3
"""Native group/library/ZIP acceptance using isolated synthetic content."""
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import time
import zipfile

ROOT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('support_smoke',ROOT/'scripts/support-smoke.py')
smoke=importlib.util.module_from_spec(spec);spec.loader.exec_module(smoke)
DATA=pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert DATA.is_relative_to(ROOT/'var'), 'Use isolated app-owned test data'
CONFIG=json.loads((ROOT/'src-tauri/tauri.conf.json').read_text())
APP_DATA=DATA/CONFIG['identifier']
OUTPUT=ROOT/'var/library-smoke';OUTPUT.mkdir(parents=True,exist_ok=True)

def click(selector):smoke.execute('document.querySelector(arguments[0]).click()',[selector])
def value(selector,value):smoke.execute('const el=document.querySelector(arguments[0]);el.value=arguments[1];el.dispatchEvent(new Event("input",{bubbles:true}));el.dispatchEvent(new Event("change",{bubbles:true}));',[selector,value])
def ready():smoke.wait(lambda:smoke.execute('return !document.querySelector("#browse-submit").disabled'))
def dialog(title):return smoke.wait(lambda:next((window for window,name in smoke.visible_window_titles() if title in name),None),20,title)
def choose(window,path):
    smoke.dialog_key(window,'ctrl+l')
    subprocess.run(['xdotool','type','--clearmodifiers',str(path)],check=True)
    subprocess.run(['xdotool','key','Return'],check=True)

def main():
    results={}
    try:
        smoke.start_app();ready()
        template=json.loads((ROOT/'fixtures/thread.json').read_text())
        for ident,title in [('t3_librarytest','Protected library post'),('t3_expiringtest','Old unsaved post')]:
            thread=dict(template,id=ident,title=title,subreddit='soccer',captured_at=int(time.time())-7200,
                        comments=[{'id':'t1_libraryreply','parent_id':ident,'body':'A saved nested discussion needle.'}],missing=[],source='proxy',comments_status='partial_proxy',link_url='https://v.redd.it/clip123')
            smoke.invoke('import_json',{'json':json.dumps(thread)})
        directory=APP_DATA/'media/t3_librarytest';directory.mkdir(parents=True,exist_ok=True)
        shutil.copy2(ROOT/'var/compact-fixture.mp4',directory/'media.mp4')
        (directory/'complete.json').write_text(json.dumps({'sources':['https://v.redd.it/clip123/HLSPlaylist.m3u8'],'files':['media.mp4']}))
        smoke.stop_app();smoke.start_app();ready()
        click('[data-thread-id="t3_librarytest"] .thread-open')
        smoke.wait(lambda:smoke.execute('return !!document.querySelector("#save-post-collection")'))
        assert smoke.execute('return document.querySelector("#save-post-forever").checked')
        value('#save-post-collection','Matches')
        click('.reader-save-panel .button.primary')
        smoke.wait(lambda:smoke.execute('return document.querySelector("#save-post-status").textContent.startsWith("Saved")'))
        click('#settings-open')
        existing=smoke.invoke('library_state')['groups']
        if existing:value('#group-select',existing[0]['id'])
        value('#group-name','Football')
        value('#group-subreddits','soccer, denmark')
        value('#group-post-hours','1')
        value('#group-media-hours','48')
        click('#group-save')
        smoke.wait(lambda:len(smoke.invoke('library_state')['groups'])==1)
        state=smoke.invoke('library_state')
        assert state['groups'][0]['post_cache_hours']==1 and state['groups'][0]['watched_media_hours']==48
        snapshot=smoke.invoke('snapshot')
        assert [thread['id'] for thread in snapshot['threads']]==['t3_librarytest']
        assert (directory/'media.mp4').is_file()
        smoke.wait(lambda:smoke.execute('return !document.querySelector("[data-thread-id=t3_expiringtest]")'))
        click('#settings-close');click('#tab-library')
        smoke.wait(lambda:smoke.execute('return !!document.querySelector("[data-library-id=t3_librarytest]")'))
        assert smoke.execute('return document.querySelector("[data-library-id=t3_librarytest]").textContent.includes("keep forever")')
        value('#library-search','discussion needle')
        smoke.wait(lambda:smoke.execute('return document.querySelector("[data-library-id=t3_librarytest]")?.textContent.includes("Comment match")'))
        click('[data-library-id="t3_librarytest"] .button.secondary')
        smoke.wait(lambda:smoke.execute('return document.querySelector("#thread-view").textContent.includes("saved nested discussion")'))
        assert smoke.execute('return !!document.querySelector("[data-comment-id=t1_libraryreply].anchor-target")')
        assert smoke.proxy_requests(smoke.invoke('app_diagnostics'))==0
        results['save_group_ttl_protection_and_offline_open']=True
        click('#tab-library')
        archive=OUTPUT/f'library-{int(time.time()*1000)}.zip'
        click('#library-export');choose(dialog('Export saved library'),archive.with_suffix(''))
        smoke.wait(archive.is_file,20,'native saved-library export')
        with zipfile.ZipFile(archive) as bundle:
            assert bundle.testzip() is None
            assert bundle.read('media/t3_librarytest/media.mp4')==(directory/'media.mp4').read_bytes()
            manifest=json.loads(bundle.read('manifest.json'))
            assert manifest['items'][0]['keep_forever']
        results['native_zip_export_with_media_bytes']=True
        restored=OUTPUT/f'restored-{int(time.time()*1000)}'
        executable=ROOT/'src-tauri/target/debug/offline-reader-desktop'
        subprocess.run([str(executable),'--cli','--data-dir',str(restored),'library','import',str(archive)],env={key:value for key,value in os.environ.items() if key!='DISPLAY'},check=True,capture_output=True,text=True)
        listing=subprocess.run([str(executable),'--cli','--data-dir',str(restored),'library','list'],check=True,capture_output=True,text=True)
        imported=json.loads(listing.stdout)
        assert imported['items'][0]['thread']['title']=='Protected library post'
        assert imported['groups'][0]['name']=='Football'
        assert (restored/'media/t3_librarytest/media.mp4').read_bytes()==(directory/'media.mp4').read_bytes()
        results['headless_packaged_cli_restore']=True
        # Native import reuses identical media and preserves the existing protection.
        click('#library-import');window=dialog('Import Hrungnir library');choose(window,archive)
        time.sleep(.3)
        if any(candidate==window for candidate,_ in smoke.visible_window_titles()):smoke.dialog_key(window,'Return')
        smoke.wait(lambda:smoke.execute('return document.querySelector("#library-status").textContent.includes("Library bundle imported")'))
        assert smoke.invoke('library_state')['items'][0]['keep_forever']
        results['native_zip_import']=True
        smoke.invoke('prune_cache',{'before':253402300800})
        assert smoke.invoke('snapshot')['threads'][0]['id']=='t3_librarytest'
        smoke.stop_app();smoke.start_app();ready();click('#tab-library')
        smoke.wait(lambda:smoke.execute('return !!document.querySelector("[data-library-id=t3_librarytest]")'))
        assert smoke.invoke('library_state')['groups'][0]['name']=='Football'
        assert (directory/'media.mp4').is_file()
        results['protection_groups_and_library_survive_restart']=True
        linked_bundle=os.environ.get('HRUNGNIR_LINKED_BUNDLE')
        if linked_bundle:
            value('#library-search','')
            click('#library-import');window=dialog('Import Hrungnir library');choose(window,pathlib.Path(linked_bundle))
            time.sleep(.3)
            if any(candidate==window for candidate,_ in smoke.visible_window_titles()):smoke.dialog_key(window,'Return')
            smoke.wait(lambda:smoke.execute('return document.querySelector("#library-status").textContent.includes("Library bundle imported")'))
            linked={}
            for kind in ['article','image','video']:
                value('#library-kind-filter',kind)
                smoke.wait(lambda:smoke.execute('return !!document.querySelector("#library-items .button.secondary")'))
                click('#library-items .button.secondary')
                smoke.wait(lambda:smoke.execute('return document.querySelector("#library-status").textContent.includes("Opened saved content")'))
                if kind=='article':
                    assert len(smoke.execute('return document.querySelector("#library-detail .thread-body").textContent'))>80
                elif kind=='image':
                    smoke.wait(lambda:smoke.execute('return document.querySelector("#library-detail img")?.naturalWidth > 0'))
                else:
                    assert smoke.execute('const v=document.querySelector("#library-detail video"); return !!v && v.src.startsWith("http://127.0.0.1:") && v.controls && !v.autoplay')
                linked[kind]=True
            results['restored_article_image_and_video_open_offline']=linked
            value('#library-kind-filter','')
        assert smoke.proxy_requests(smoke.invoke('app_diagnostics'))==0
        smoke.screenshot(OUTPUT/'library.png')
        results.update(status='passed',platform='Linux Tauri/WebKitGTK; native Mac pending')
        (OUTPUT/'result.json').write_text(json.dumps(results,indent=2));print(json.dumps(results),flush=True)
    except Exception:
        print('Native library failure:',smoke.execute('return {library:document.querySelector("#library-status").textContent,group:document.querySelector("#group-status").textContent}'),flush=True)
        smoke.screenshot(OUTPUT/'failure.png')
        raise
    finally:smoke.stop_app()

if __name__=='__main__':main()
