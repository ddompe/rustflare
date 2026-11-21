#!/usr/bin/env python3
"""Tiny stand-in for the GitHub Releases API, to test `rustflare-update` without GitHub.

  scripts/mock-github-release.py --deb rustflare_0.1.0-3_arm64.deb --sums SHA256SUMS --port 9999
  RUSTFLARE_API_URL=http://127.0.0.1:9999 rustflare-update --check
"""
import argparse, http.server, json, os

ap = argparse.ArgumentParser()
ap.add_argument("--deb", required=True)
ap.add_argument("--sums")
ap.add_argument("--port", type=int, default=9999)
ap.add_argument("--repo", default="ddompe/rustflare")
a = ap.parse_args()

files = {os.path.basename(a.deb): a.deb}
if a.sums:
    files["SHA256SUMS"] = a.sums
version = os.path.basename(a.deb).split("_")[1]


class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        base = f"http://127.0.0.1:{a.port}"
        if self.path == f"/repos/{a.repo}/releases/latest":
            body = json.dumps({
                "tag_name": "v" + version.split("-")[0],
                "assets": [{"name": n, "url": f"{base}/assets/{n}"} for n in files],
            }).encode()
            ctype = "application/json"
        elif self.path.startswith("/assets/") and self.path[8:] in files:
            body = open(files[self.path[8:]], "rb").read()
            ctype = "application/octet-stream"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


http.server.HTTPServer(("127.0.0.1", a.port), H).serve_forever()
