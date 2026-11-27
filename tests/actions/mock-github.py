#!/usr/bin/env python3
"""The parts of the GitHub REST API `changesetter version-pr --provider github`
calls, backed by files in a fixture directory. Ancestry for /compare comes
from the real bare origin. Writes the port it listens on to <fixtures>/port.

Usage: mock-github.py <fixtures dir> <origin git dir>

Fixtures read: commit-<sha>.json (PRs for a commit), pulls.json (closed
version PRs), open.json (open version PRs), releases.txt (existing release
tags, a tab and "draft" for a draft), labels.txt. Written: created.log (one JSON line per created release),
pr-body.md (the last version PR body), calls.log. FAIL_API=1 answers 401.
"""
import json
import os
import subprocess
import sys
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

FX, ORIGIN = sys.argv[1], sys.argv[2]
PREFIX = "/repos/o/r/"


def read(name, default):
    try:
        with open(os.path.join(FX, name)) as f:
            return f.read()
    except FileNotFoundError:
        return default


def append(name, line):
    with open(os.path.join(FX, name), "a") as f:
        f.write(line + "\n")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, body):
        data = body if isinstance(body, str) else json.dumps(body)
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data.encode())))
        self.end_headers()
        self.wfile.write(data.encode())

    def handle_any(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(length) or "null")
        append("calls.log", f"{self.command} {self.path}")
        if os.environ.get("FAIL_API"):
            return self.reply(401, {"message": "Bad credentials"})
        url = urllib.parse.urlsplit(self.path)
        path = urllib.parse.unquote(url.path)
        query = urllib.parse.parse_qs(url.query)
        if not path.startswith(PREFIX):
            return self.reply(404, {"message": "Not Found"})
        rest = path[len(PREFIX):]
        method = self.command

        if method == "GET" and rest.startswith("commits/") and rest.endswith("/pulls"):
            sha = rest[len("commits/"):-len("/pulls")]
            return self.reply(200, read(f"commit-{sha}.json", "[]"))
        if method == "GET" and rest == "pulls":
            # Like GitHub, `head` filters by owner:branch.
            head = query.get("head", [""])[0]
            if head != "o:changesetter/version-packages":
                return self.reply(200, [])
            state = query.get("state", [""])[0]
            return self.reply(200, read("pulls.json" if state == "closed" else "open.json", "[]"))
        if method == "GET" and rest.startswith("compare/"):
            base, head = rest[len("compare/"):].split("...")
            ancestor = subprocess.run(
                ["git", "-C", ORIGIN, "merge-base", "--is-ancestor", base, head]
            ).returncode == 0
            return self.reply(200, {"status": "ahead" if ancestor else "diverged"})
        releases = [line.split("\t") for line in read("releases.txt", "").splitlines() if line]
        if method == "GET" and rest.startswith("releases/tags/"):
            # Like GitHub: this endpoint only finds published releases.
            tag = rest[len("releases/tags/"):]
            exists = any(r[0] == tag and len(r) == 1 for r in releases)
            return self.reply(200 if exists else 404, {} if exists else {"message": "Not Found"})
        if method == "GET" and rest == "releases":
            page = int(query.get("page", ["1"])[0])
            listed = [{"tag_name": r[0], "draft": len(r) > 1} for r in reversed(releases)]
            return self.reply(200, listed if page == 1 else [])
        if method == "POST" and rest == "releases":
            # GitHub would create a missing tag at target_commitish; the CLI
            # always pushes the tag first, so a missing one is a bug here.
            tag_ok = subprocess.run(
                ["git", "-C", ORIGIN, "rev-parse", "--verify", "-q", f"refs/tags/{body['tag_name']}"],
                capture_output=True,
            ).returncode == 0
            if not tag_ok or not body.get("target_commitish"):
                return self.reply(422, {"message": "tag missing or no target_commitish"})
            append("releases.txt", body["tag_name"] + ("\tdraft" if body.get("draft") else ""))
            append("created.log", json.dumps(body))
            return self.reply(201, {"html_url": f"https://github.test/releases/{body['tag_name']}"})
        if method == "POST" and rest == "pulls":
            if read("open.json", "[]").strip() not in ("", "[]"):
                return self.reply(422, {"message": "A pull request already exists"})
            with open(os.path.join(FX, "pr-body.md"), "w") as f:
                f.write(body["body"])
            return self.reply(201, {"number": 1, "html_url": "https://github.test/pull/1"})
        if method == "PATCH" and rest.startswith("pulls/"):
            with open(os.path.join(FX, "pr-body.md"), "w") as f:
                f.write(body["body"])
            return self.reply(200, {"number": int(rest.split("/")[1])})
        if method == "POST" and rest == "labels":
            if body["name"] in read("labels.txt", "").splitlines():
                return self.reply(422, {"message": "Validation Failed", "errors": [{"code": "already_exists"}]})
            append("labels.txt", body["name"])
            return self.reply(201, {"name": body["name"]})
        if method == "POST" and rest.startswith("issues/") and rest.endswith("/labels"):
            return self.reply(200, [])
        return self.reply(404, {"message": f"unexpected {method} {self.path}"})

    do_GET = do_POST = do_PATCH = handle_any


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(os.path.join(FX, "port.tmp"), "w") as f:
    f.write(str(server.server_address[1]))
os.replace(os.path.join(FX, "port.tmp"), os.path.join(FX, "port"))
server.serve_forever()
