"""Serve the real reader with deterministic native API fixtures for browser checks.

Run python3 tests/reader-ux/serve.py, open http://localhost:18763 with
playwright-cli, then run-code the async function in check.js. Requests are
deliberately held open to verify loading, navigation and media independence.
This exercises the UI contract; it does not test Reddit access or playback.
"""
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT / "reader"), **kwargs)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path in ("/", "/index.html"):
            data = (ROOT / "reader/index.html").read_text().replace(
                '<script defer src="app.js">',
                '<script src="/__fixture.js"></script><script defer src="app.js">',
            ).encode()
            kind = "text/html"
        elif path == "/__fixture.js":
            data = Path(__file__).with_name("fixture.js").read_bytes()
            kind = "application/javascript"
        elif path.endswith("/thumb.png"):
            data = (ROOT / "reader/hrungnir.png").read_bytes()
            kind = "image/png"
        else:
            return super().do_GET()
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.end_headers()
        self.wfile.write(data)


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 18763), Handler).serve_forever()
