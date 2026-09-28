#!/usr/bin/env python3
"""Codex, 2026-10-21: isolated migration, authenticated HTTP and restart smoke check."""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def request(base, path, payload=None, cookie=None):
    data = None if payload is None else json.dumps(payload).encode()
    headers = {"Content-Type": "application/json"}
    if cookie:
        headers["Cookie"] = cookie
    with urllib.request.urlopen(urllib.request.Request(base + path, data=data, headers=headers), timeout=3) as response:
        return json.load(response), response.headers.get("Set-Cookie", "").split(";", 1)[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="medienserver-boot-") as temporary:
        root = Path(temporary)
        media = root / "media"
        media.mkdir()
        password = secrets.token_urlsafe(32)
        secret = root / "bootstrap-password"
        secret.write_text(password)
        secret.chmod(0o600)
        env = {k: v for k, v in os.environ.items() if not k.startswith(("TMDB_", "ADMIN_"))}
        env["RUST_LOG"] = "medienserver=info"
        for iteration in range(2):
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            base = f"http://127.0.0.1:{port}"
            command = [str(args.binary.resolve()), "--media-path", str(media), "--db-path", str(root / "test.db"),
                       "--metadata-path", str(root / "metadata"), "--port", str(port), "--bind", "127.0.0.1",
                       "--scan-interval-seconds", "0", "--admin-password-file", str(secret)]
            with (root / f"boot-{iteration}.log").open("w+") as log:
                process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
                try:
                    deadline = time.monotonic() + 20
                    while True:
                        if process.poll() is not None:
                            raise RuntimeError("server exited before becoming healthy")
                        try:
                            health, _ = request(base, "/api/v1/health")
                            assert health["status"] == "ok"
                            break
                        except (OSError, urllib.error.URLError):
                            if time.monotonic() >= deadline:
                                raise RuntimeError("server failed to become healthy")
                            time.sleep(0.05)
                    _, cookie = request(base, "/api/v1/login", {"username": "admin", "password": password})
                    profiles, _ = request(base, "/api/v1/profiles", cookie=cookie)
                    assert len(profiles["items"]) == 1 and profiles["items"][0]["is_default"]
                    for endpoint in ("shows", "movies", "continue-watching"):
                        catalog, _ = request(base, "/api/v1/" + endpoint, cookie=cookie)
                        assert catalog["total"] == 0 and catalog["items"] == []
                    try:
                        request(base, "/api/v1/shows")
                        raise AssertionError("catalog did not require authentication")
                    except urllib.error.HTTPError as error:
                        assert error.code == 401
                finally:
                    if process.poll() is None:
                        process.send_signal(signal.SIGTERM)
                        try:
                            process.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                            raise RuntimeError("server did not shut down gracefully")
                assert process.returncode == 0
                log.seek(0)
                assert "ERROR" not in log.read(), "server logged an error"
        print(json.dumps({"first_boot": "ok", "restart": "ok", "authenticated_hierarchy": "ok",
                          "default_profile": "ok", "graceful_shutdown": "ok", "production_changed": False}))


if __name__ == "__main__":
    main()
