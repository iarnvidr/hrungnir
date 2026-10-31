#!/usr/bin/env python3
"""Run the existing reader UX checks in headless Chromium through CDP.

The runner is intentionally Linux-container-only. It uses no Node runtime and
does not use WebDriver or a browser driver.
"""
from __future__ import annotations

import base64
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

import websocket


HERE = Path(__file__).resolve().parent
CHECKS = (
    "check.js",
    "navigation-check.js",
    "settings-update-check.js",
    "search-keyboard-check.js",
    "search-layout-check.js",
)


def require_container() -> None:
    if sys.platform != "linux" or os.environ.get("READER_UX_CONTAINER") != "1":
        raise SystemExit("Reader UX browser checks must run in the Linux CI container (set READER_UX_CONTAINER=1 there).")


class Cdp:
    def __init__(self, ws_url: str, browser: "Cdp | None" = None) -> None:
        self.ws = websocket.create_connection(ws_url, timeout=300, enable_multithread=True, suppress_origin=True)
        self.next_id = 0
        self.responses: dict[int, dict] = {}
        self.browser = browser
        self.pressed_keys: set[str] = set()
        self.page_errors: list[str] = []

    def command(self, method: str, params: dict | None = None, *, context: int | None = None) -> dict:
        self.next_id += 1
        request_id = self.next_id
        message = {"id": request_id, "method": method, "params": params or {}}
        if context is not None:
            message["sessionId"] = context
        self.ws.send(json.dumps(message))
        while True:
            if request_id in self.responses:
                reply = self.responses.pop(request_id)
                if "error" in reply:
                    raise RuntimeError(f"CDP {method}: {reply['error'].get('message', reply['error'])}")
                return reply.get("result", {})
            reply = json.loads(self.ws.recv())
            if reply.get("id") == request_id:
                if "error" in reply:
                    raise RuntimeError(f"CDP {method}: {reply['error'].get('message', reply['error'])}")
                return reply.get("result", {})
            if isinstance(reply.get("id"), int):
                self.responses[reply["id"]] = reply
                continue
            if reply.get("method") == "Runtime.exceptionThrown":
                details = reply.get("params", {}).get("exceptionDetails", {})
                exception = details.get("exception", {}).get("description")
                self.page_errors.append(exception or details.get("text", "uncaught browser exception"))
                continue
            if reply.get("method") == "Runtime.bindingCalled" and reply.get("params", {}).get("name") == "__readerUxBridge":
                self.handle_bridge(reply["params"])

    def handle_bridge(self, event: dict) -> None:
        request = json.loads(event["payload"])
        rpc_id, method, args = request["id"], request["method"], request.get("args", {})
        value = None
        error = None
        try:
            if method == "viewport":
                self.command("Emulation.setDeviceMetricsOverride", {
                    "width": int(args["width"]), "height": int(args["height"]),
                    "deviceScaleFactor": 1, "mobile": False,
                })
            elif method == "reload":
                self.evaluate("new Promise(resolve => { const frame = document.querySelector('#reader-fixture'); frame.addEventListener('load', () => resolve(true), {once:true}); frame.contentWindow.location.reload(); })")
                self.wait_fixture_ready()
            elif method == "navigate":
                target_url = json.dumps(args["url"])
                self.evaluate(f"new Promise(resolve => {{ const frame = document.querySelector('#reader-fixture'); frame.addEventListener('load', () => resolve(true), {{once:true}}); frame.src = {target_url}; }})")
                self.wait_fixture_ready()
            elif method == "click":
                modifiers = modifier_mask(args.get("modifiers", []))
                x, y = float(args["x"]), float(args["y"])
                self.command("Input.dispatchMouseEvent", {"type": "mouseMoved", "x": x, "y": y, "modifiers": modifiers})
                self.command("Input.dispatchMouseEvent", {"type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1, "modifiers": modifiers})
                self.command("Input.dispatchMouseEvent", {"type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1, "modifiers": modifiers})
            elif method == "key":
                dispatch_key(self, args)
            elif method == "type":
                for character in args["text"]:
                    dispatch_key(self, {"key": character, "modifiers": ["shift"] if character.isupper() else [], "action": "press"})
            elif method == "insertText":
                self.command("Input.insertText", {"text": args["text"]})
            elif method == "permissions":
                permissions = {"clipboard-read": "clipboardReadWrite", "clipboard-write": "clipboardSanitizedWrite"}
                (self.browser or self).command("Browser.grantPermissions", {
                    "origin": self.evaluate("location.origin"),
                    "permissions": [permissions.get(item, item) for item in args["permissions"]],
                })
            elif method == "screenshot":
                result = self.command("Page.captureScreenshot", {"format": "png", "captureBeyondViewport": True})
                Path(args["path"]).write_bytes(base64.b64decode(result["data"]))
                value = True
            else:
                raise ValueError(f"unknown browser operation: {method}")
        except Exception as exc:  # Return host-side details as a failed page promise.
            error = str(exc)
        encoded = json.dumps(value)
        error_arg = json.dumps(error)
        expression = f"window.__resolveReaderUxHost({rpc_id}, {encoded}, {error_arg})"
        self.command("Runtime.evaluate", {"expression": expression, "awaitPromise": True})

    def evaluate(self, expression: str, *, await_promise: bool = True) -> object:
        result = self.command("Runtime.evaluate", {
            "expression": expression,
            "returnByValue": True,
            "awaitPromise": await_promise,
            "userGesture": True,
        })
        if "exceptionDetails" in result:
            details = result["exceptionDetails"]
            description = details.get("exception", {}).get("description") or details.get("text")
            raise RuntimeError(description or "page evaluation failed")
        return result.get("result", {}).get("value")

    def wait_ready(self) -> None:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            try:
                if self.evaluate("document.readyState === 'complete'"):
                    return
            except Exception:
                pass
            time.sleep(0.05)
        raise TimeoutError("Chromium runner page did not finish loading")

    def wait_fixture_ready(self) -> None:
        deadline = time.monotonic() + 20
        expression = "(() => { const frame = document.querySelector('#reader-fixture'); return !!frame && frame.contentDocument?.readyState === 'complete' })()"
        while time.monotonic() < deadline:
            try:
                if self.evaluate(expression):
                    return
            except Exception:
                pass
            time.sleep(0.05)
        raise TimeoutError("Reader fixture frame did not finish loading")

    def close(self) -> None:
        self.ws.close()


def modifier_mask(modifiers: list[str]) -> int:
    mask = 0
    for modifier in modifiers:
        mask |= {"alt": 1, "control": 2, "ctrl": 2, "meta": 4, "command": 4, "shift": 8}.get(modifier.lower(), 0)
    return mask


def key_info(key: str) -> tuple[str, int]:
    if len(key) == 1 and key.isalpha():
        return f"Key{key.upper()}", ord(key.upper())
    if len(key) == 1 and key.isdigit():
        return f"Digit{key}", ord(key)
    codes = {
        "Escape": ("Escape", 27), "Enter": ("Enter", 13), "Tab": ("Tab", 9), " ": ("Space", 32),
        "ArrowLeft": ("ArrowLeft", 37), "ArrowUp": ("ArrowUp", 38), "ArrowRight": ("ArrowRight", 39), "ArrowDown": ("ArrowDown", 40),
        "Home": ("Home", 36), "End": ("End", 35), "PageUp": ("PageUp", 33), "PageDown": ("PageDown", 34),
        "Backspace": ("Backspace", 8), "Delete": ("Delete", 46),
        ",": ("Comma", 188), ".": ("Period", 190), "/": ("Slash", 191), "?": ("Slash", 191),
        ";": ("Semicolon", 186), ":": ("Semicolon", 186), "'": ("Quote", 222), '"': ("Quote", 222),
        "-": ("Minus", 189), "_": ("Minus", 189), "=": ("Equal", 187), "+": ("Equal", 187),
    }
    if key in codes:
        return codes[key]
    if key in ("Control", "Ctrl"):
        return "ControlLeft", 17
    if key in ("Meta", "Command"):
        return "MetaLeft", 91
    if key == "Shift":
        return "ShiftLeft", 16
    if key == "Alt":
        return "AltLeft", 18
    return key, 0


def dispatch_key(cdp: Cdp, args: dict) -> None:
    key, modifiers, action = args["key"], args.get("modifiers", []), args["action"]
    modifier_keys = []
    for item in modifiers:
        name = {"control": "Control", "ctrl": "Control", "command": "Meta", "meta": "Meta", "shift": "Shift", "alt": "Alt"}.get(item.lower())
        if name and name not in modifier_keys:
            modifier_keys.append(name)
    code, vk = key_info(key)
    bitmask = modifier_mask(modifiers)
    def send(event_type: str, event_key: str, event_code: str, virtual_key: int, flags: int, repeat: bool = False) -> None:
        payload = {"type": event_type, "key": event_key, "code": event_code, "windowsVirtualKeyCode": virtual_key, "nativeVirtualKeyCode": virtual_key, "modifiers": flags}
        if event_type == "char":
            payload["text"] = event_key
            payload["unmodifiedText"] = event_key
        if repeat:
            payload["autoRepeat"] = True
        cdp.command("Input.dispatchKeyEvent", payload)
    if action == "press":
        for name in modifier_keys:
            mod_code, mod_vk = key_info(name)
            send("rawKeyDown", name, mod_code, mod_vk, bitmask)
        send("keyDown", key, code, vk, bitmask)
        if len(key) == 1 and key not in (",", "."):
            send("char", key, code, vk, bitmask)
        send("keyUp", key, code, vk, bitmask)
        for name in reversed(modifier_keys):
            mod_code, mod_vk = key_info(name)
            send("keyUp", name, mod_code, mod_vk, bitmask)
    elif action == "down":
        send("keyDown", key, code, vk, bitmask, repeat=key in cdp.pressed_keys)
        cdp.pressed_keys.add(key)
    elif action == "up":
        send("keyUp", key, code, vk, bitmask)
        cdp.pressed_keys.discard(key)

def http_json(url: str, method: str = "GET") -> object:
    request = urllib.request.Request(url, method=method)
    with urllib.request.urlopen(request, timeout=3) as response:
        return json.loads(response.read())


def wait_for_devtools(port_file: Path, process: subprocess.Popen) -> tuple[int, str]:
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Chromium exited before DevTools was ready (exit {process.returncode})")
        if port_file.exists():
            lines = port_file.read_text().splitlines()
            if len(lines) >= 2:
                return int(lines[0]), lines[1]
        time.sleep(0.05)
    raise TimeoutError("Chromium DevTools endpoint did not become ready")


def start_fixture():
    spec = importlib.util.spec_from_file_location("reader_ux_fixture_server", HERE / "serve.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    server = module.ThreadingHTTPServer(("127.0.0.1", 0), module.Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, f"http://127.0.0.1:{server.server_port}/"


def main() -> int:
    require_container()
    chromium = os.environ.get("CHROMIUM_BIN") or shutil.which("chromium") or shutil.which("chromium-browser")
    if not chromium:
        raise SystemExit("Chromium executable not found; set CHROMIUM_BIN in the CI container.")

    server, url = start_fixture()
    browser = None
    cdp = None
    browser_cdp = None
    try:
        with tempfile.TemporaryDirectory(prefix="reader-ux-chromium-") as profile:
            profile_path = Path(profile)
            command = [
                chromium, "--headless=new", "--no-sandbox", "--disable-dev-shm-usage",
                "--no-first-run", "--no-default-browser-check", "--disable-background-networking",
                "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1",
                f"--user-data-dir={profile}", "about:blank",
            ]
            browser = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            port, browser_ws_path = wait_for_devtools(profile_path / "DevToolsActivePort", browser)
            browser_ws = browser_ws_path if browser_ws_path.startswith("ws://") else f"ws://127.0.0.1:{port}{browser_ws_path}"
            deadline = time.monotonic() + 10
            targets = []
            while time.monotonic() < deadline:
                try:
                    targets = http_json(f"http://127.0.0.1:{port}/json/list")
                    break
                except Exception:
                    time.sleep(0.05)
            target = next((item for item in targets if item.get("type") == "page"), None)
            if target is None:
                raise RuntimeError("Chromium did not expose a page target")
            browser_cdp = Cdp(browser_ws)
            cdp = Cdp(target["webSocketDebuggerUrl"], browser_cdp)
            cdp.command("Page.enable")
            cdp.command("Runtime.enable")
            cdp.command("Runtime.addBinding", {"name": "__readerUxBridge"})
            cdp.command("Emulation.setDeviceMetricsOverride", {"width": 1280, "height": 900, "deviceScaleFactor": 1, "mobile": False})
            bootstrap = (HERE / "browser-facade.js").read_text()
            cdp.command("Page.addScriptToEvaluateOnNewDocument", {"source": bootstrap})
            cdp.command("Page.navigate", {"url": url + "__runner.html"})
            cdp.wait_ready()
            cdp.wait_fixture_ready()

            for filename in CHECKS:
                source = (HERE / filename).read_text()
                expression = "(async () => { const check = (" + source + "); return await check(window.__readerUxPage); })()"
                cdp.page_errors.clear()
                result = cdp.evaluate(expression)
                if cdp.page_errors:
                    raise RuntimeError(f"{filename}: page errors: " + "\n".join(cdp.page_errors))
                passed = result.get("passed", []) if isinstance(result, dict) else []
                print(f"{filename}: {len(passed)} groups passed; " + ", ".join(passed), flush=True)
        return 0
    finally:
        if cdp:
            cdp.close()
        if browser_cdp:
            browser_cdp.close()
        if browser and browser.poll() is None:
            browser.terminate()
            try:
                browser.wait(timeout=5)
            except subprocess.TimeoutExpired:
                browser.kill()
                browser.wait()
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"reader UX browser checks failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
