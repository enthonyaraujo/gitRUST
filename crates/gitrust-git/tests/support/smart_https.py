"""Loopback smart Git HTTP server for the remote operation fixtures."""
import base64
import os
import ssl
import subprocess
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit


class Handler(BaseHTTPRequestHandler):
    def handle(self):
        try:
            super().handle()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            pass

    def log_message(self, *_):
        pass

    def do_GET(self):
        self.serve_git()

    def do_POST(self):
        self.serve_git()

    def read_body(self):
        if self.headers.get("Transfer-Encoding") != "chunked":
            return self.rfile.read(int(self.headers.get("Content-Length", 0)))
        chunks = []
        while length := int(self.rfile.readline().split(b";", 1)[0], 16):
            chunks.append(self.rfile.read(length))
            self.rfile.read(2)
        return b"".join(chunks)

    def serve_git(self):
        url = urlsplit(self.path)
        with open("requests", "a", encoding="ascii") as requests:
            requests.write(url.path + "\n")
        if url.path.startswith("/slow.git/"):
            time.sleep(15)
        if url.path.startswith("/private.git/"):
            expected = b"Basic " + base64.b64encode(b"fixture:fixture-password")
            if self.headers.get("Authorization", "").encode("ascii") != expected:
                self.send_response(401)
                self.send_header("WWW-Authenticate", 'Basic realm="fixture"')
                self.end_headers()
                return
        if url.path.startswith("/protected.git/") and (
            url.query == "service=git-receive-pack" or url.path.endswith("/git-receive-pack")
        ):
            self.send_response(403)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        body = self.read_body()
        output = subprocess.run(
            [sys.argv[1], "http-backend"], input=body, capture_output=True,
            env={
                "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_PROJECT_ROOT": os.getcwd(), "GIT_HTTP_EXPORT_ALL": "1",
                "GIT_PROTOCOL": self.headers.get("Git-Protocol", ""),
                "REQUEST_METHOD": self.command, "PATH_INFO": url.path,
                "QUERY_STRING": url.query, "REMOTE_USER": "fixture",
                "CONTENT_LENGTH": str(len(body)),
                "CONTENT_TYPE": self.headers.get("Content-Type", ""),
            },
            check=False,
        ).stdout
        headers, _, response = output.partition(b"\r\n\r\n")
        headers = [line.decode("latin1").split(":", 1) for line in headers.split(b"\r\n") if b":" in line]
        status = next((int(value.split()[0]) for key, value in headers if key == "Status"), 200)
        self.send_response(status)
        for key, value in headers:
            if key.lower() not in ("status", "content-length"):
                self.send_header(key, value.strip())
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        try:
            self.wfile.write(response)
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            pass


with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain("fixture.crt", "fixture.key")
    server.socket = context.wrap_socket(server.socket, server_side=True)
    print(server.server_port, flush=True)
    server.serve_forever()
