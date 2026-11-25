#!/usr/bin/env python3
"""A minimal in-memory double of the Garage admin API v2 (HSP-269).

It implements only the endpoints charts/hodei-artifacts/templates/garage-bootstrap.yaml
calls: GetClusterStatus, UpdateClusterLayout, ApplyClusterLayout, GetBucketInfo,
CreateBucket, GetKeyInfo, ImportKey and AllowBucketKey. Every request is appended
to a log file as "METHOD /path" so a test can assert which mutations happened.

Usage: garage-admin-double.py PORT_FILE REQUEST_LOG TOKEN [--fail ENDPOINT=STATUS]...
It binds 127.0.0.1 on an ephemeral port, writes the port to PORT_FILE and serves
until it is terminated. No external network is used.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

NODE_ID = "ab" * 32
BUCKET_ID = "cd" * 32

state = {"role": None, "layout_version": 0, "buckets": {}, "keys": set()}
port_file, request_log, token = sys.argv[1:4]
failures = {}
rest = sys.argv[4:]
while rest:
    if rest[0] != "--fail" or len(rest) < 2:
        sys.exit("bad arguments")
    endpoint, status = rest[1].split("=")
    failures[endpoint] = int(status)
    rest = rest[2:]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, body):
        data = json.dumps(body, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def handle_any(self, method):
        url = urlparse(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(length) or b"{}")
        query = parse_qs(url.query)
        name = url.path.rsplit("/", 1)[-1]
        with open(request_log, "a") as log:
            log.write(f"{method} {url.path}\n")
        if self.headers.get("Authorization") != f"Bearer {token}":
            return self.reply(403, {"message": "bad token"})
        if not url.path.startswith("/v2/"):
            return self.reply(404, {"message": "unknown route"})
        if name in failures:
            return self.reply(failures[name], {"message": "injected failure"})
        if name == "GetClusterStatus" and method == "GET":
            role = state["role"]
            node = {"id": NODE_ID, "isUp": True, "role": role}
            return self.reply(200, {"layoutVersion": state["layout_version"], "nodes": [node]})
        if name == "UpdateClusterLayout" and method == "POST":
            roles = body.get("roles") or []
            if not roles or not all(r.get("id") and r.get("zone") and r.get("capacity") for r in roles):
                return self.reply(400, {"message": "bad roles"})
            state["staged"] = roles[0]
            return self.reply(200, {"version": state["layout_version"] + 1})
        if name == "ApplyClusterLayout" and method == "POST":
            if body.get("version") != state["layout_version"] + 1 or "staged" not in state:
                return self.reply(400, {"message": "bad version"})
            staged = state.pop("staged")
            state["role"] = {"zone": staged["zone"], "capacity": staged["capacity"], "tags": []}
            state["layout_version"] += 1
            return self.reply(200, {"layout": {"version": state["layout_version"]}})
        if name == "GetBucketInfo" and method == "GET":
            alias = query.get("globalAlias", [""])[0]
            if alias in state["buckets"]:
                return self.reply(200, {"id": state["buckets"][alias], "globalAliases": [alias]})
            return self.reply(404, {"message": "no such bucket"})
        if name == "CreateBucket" and method == "POST":
            alias = body.get("globalAlias")
            if not alias:
                return self.reply(400, {"message": "no alias"})
            state["buckets"][alias] = BUCKET_ID
            return self.reply(200, {"id": BUCKET_ID, "globalAliases": [alias]})
        if name == "GetKeyInfo" and method == "GET":
            key = query.get("id", [""])[0]
            if key in state["keys"]:
                return self.reply(200, {"accessKeyId": key})
            return self.reply(404, {"message": "no such key"})
        if name == "ImportKey" and method == "POST":
            if not (body.get("accessKeyId") and body.get("secretAccessKey")):
                return self.reply(400, {"message": "bad key"})
            state["keys"].add(body["accessKeyId"])
            return self.reply(200, {"accessKeyId": body["accessKeyId"]})
        if name == "AllowBucketKey" and method == "POST":
            if body.get("bucketId") != BUCKET_ID or body.get("accessKeyId") not in state["keys"]:
                return self.reply(400, {"message": "bad grant"})
            return self.reply(200, {"id": BUCKET_ID})
        return self.reply(404, {"message": "unknown endpoint"})

    def do_GET(self):
        self.handle_any("GET")

    def do_POST(self):
        self.handle_any("POST")


server = HTTPServer(("127.0.0.1", 0), Handler)
with open(port_file, "w") as handle:
    handle.write(str(server.server_address[1]))
server.serve_forever()
