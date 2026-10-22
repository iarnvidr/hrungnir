#!/usr/bin/env python3
"""Native Linux isolated link-window and close accelerator check.

Run with a private tauri-driver and XDG_DATA_HOME beneath this app's var/.
Review the desktop PNG: WebKitGTK WebDriver cannot inspect incognito pages.
"""
import importlib.util
import json
import os
import pathlib
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('support_smoke', ROOT / 'scripts/support-smoke.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
assert pathlib.Path(os.environ['XDG_DATA_HOME']).resolve().is_relative_to(ROOT / 'var')
OUT = ROOT / 'var/web-link-smoke'
OUT.mkdir(parents=True, exist_ok=True)


def windows():
    s.execute('window.__nativeWindowLabels=null;window.__TAURI__.webviewWindow.getAllWebviewWindows().then(w=>window.__nativeWindowLabels=w.map(x=>x.label))')
    return s.wait(lambda:s.execute('return window.__nativeWindowLabels'))


try:
    s.start_app()
    s.wait(lambda:s.execute('return !document.querySelector("#browse-submit").disabled'))
    label = s.invoke('open_web_link', {'url':'https://example.org/'})
    assert label.startswith('web-link-') and label in windows()
    assert s.execute('return location.href.startsWith("tauri:") && !!document.querySelector("#browse-submit")')
    time.sleep(3)
    import gi
    gi.require_version('Gdk','3.0')
    from gi.repository import Gdk
    desktop = Gdk.get_default_root_window()
    Gdk.pixbuf_get_from_window(desktop,0,0,desktop.get_width(),desktop.get_height()).savev(str(OUT / 'browser.png'),'png',[],[])
    subprocess.run(['xdotool','key','ctrl+w'],check=True)
    s.wait(lambda:label not in windows())
    assert s.execute('return !!document.querySelector("#browse-submit")')
    result = {'native_window_and_close':'passed','reader_context_preserved':True,
              'page_rendering':'Review browser.png; incognito page not exposed by WebKitGTK WebDriver',
              'remote_ipc_denial':'Capability configuration reviewed; remote-page invoke not exercised',
              'platform':'Linux; native macOS pending'}
    (OUT / 'result.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result),flush=True)
finally:
    s.stop_app()
