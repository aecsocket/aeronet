#!/usr/bin/env python3
"""Run the browser WebRTC harness against the native JSON-line harness.

The page reports success only after both peers have opened a session, exchanged
packets over the real DataChannel, classified a direct UDP path, and completed
explicit disconnect cleanup.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from collections import deque
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from subprocess import Popen
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
NATIVE_EXAMPLE = ROOT / "target" / "debug" / "examples" / "browser_native_harness"
WASM_EXAMPLE = (
    ROOT
    / "target"
    / "wasm32-unknown-unknown"
    / "debug"
    / "examples"
    / "browser_wasm_harness.wasm"
)


def target_path(environment: dict[str, str], default: Path) -> Path:
    configured = environment.get("CARGO_TARGET_DIR")
    if configured is None:
        return default
    path = Path(configured)
    return path if path.is_absolute() else ROOT / path


def run(command: list[str], environment: dict[str, str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=environment, check=True)


def build_harnesses(environment: dict[str, str], assets: Path) -> None:
    build_environment = environment.copy()
    run(
        [
            "cargo",
            "build",
            "-p",
            "aeronet_webrtc",
            "--example",
            "browser_native_harness",
            "--features",
            "server",
        ],
        build_environment,
    )
    run(
        [
            "cargo",
            "build",
            "-p",
            "aeronet_webrtc",
            "--target",
            "wasm32-unknown-unknown",
            "--example",
            "browser_wasm_harness",
            "--features",
            "client",
        ],
        build_environment,
    )
    wasm_bindgen = shutil.which("wasm-bindgen")
    if wasm_bindgen is None:
        raise RuntimeError("wasm-bindgen-cli is required to run the browser harness")
    wasm_path = target_path(
        build_environment, ROOT / "target"
    ) / WASM_EXAMPLE.relative_to(ROOT / "target")
    if not wasm_path.is_file():
        raise RuntimeError(f"WASM harness was not built: {wasm_path}")
    run(
        [
            wasm_bindgen,
            "--target",
            "web",
            "--no-typescript",
            "--out-dir",
            str(assets),
            str(wasm_path),
        ],
        build_environment,
    )


PAGE = """<!doctype html>
<meta charset="utf-8">
<title>Aeronet WebRTC harness</title>
<pre id="result">starting</pre>
<script type="module">
import init, * as harness from "/browser_wasm_harness.js";

const output = document.querySelector("#result");
const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const json = async (url, options) => {
  const response = await fetch(url, options);
  if (!response.ok) throw new Error(`${url} returned HTTP ${response.status}`);
  return response.json();
};
const post = (url, value) => json(url, {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(value),
});

async function tick(native) {
  const signal = await json("/native/signal");
  if (signal !== null) harness.receive_signal(JSON.stringify(signal));
  const browser = JSON.parse(harness.tick());
  for (const localSignal of browser.signals) await post("/native/signal", localSignal);
  await post("/browser/status", browser);
  const nativeState = await json("/native/status");
  if (nativeState.error) throw new Error(`native harness: ${nativeState.error}`);
  return { browser, native: nativeState.status ?? native };
}

async function main() {
  try {
    await init();
    harness.start();
    const exchangeDeadline = Date.now() + 60_000;
    let snapshot;
    while (Date.now() < exchangeDeadline) {
      snapshot = await tick(null);
      const { browser, native } = snapshot;
      if (browser.error) throw new Error(`browser harness: ${browser.error}`);
      if (
        browser.endpoint_alive
        && browser.session_open
        && browser.pong_received
        && browser.path === "direct/udp"
        && native?.endpoint_alive
        && native.session_open
        && native.packet_exchange
        && native.path === "direct/udp"
      ) break;
      await sleep(10);
    }
    if (
      !snapshot?.browser.session_open
      || !snapshot.browser.pong_received
      || snapshot.browser.path !== "direct/udp"
      || !snapshot.native?.session_open
      || !snapshot.native.packet_exchange
      || snapshot.native.path !== "direct/udp"
    ) throw new Error(`exchange timeout: ${JSON.stringify(snapshot)}`);

    const connectedSnapshot = snapshot;
    harness.cancel();
    const cleanupDeadline = Date.now() + 10_000;
    while (Date.now() < cleanupDeadline) {
      snapshot = await tick(snapshot.native);
      if (
        !snapshot.browser.endpoint_alive
        && snapshot.browser.disconnected === "browser harness cancelled"
        && snapshot.native?.endpoint_alive === false
        && snapshot.native.disconnected === "data channel closed"
      ) break;
      await sleep(10);
    }
    if (
      snapshot.browser.endpoint_alive
      || snapshot.browser.disconnected !== "browser harness cancelled"
      || snapshot.native?.endpoint_alive !== false
      || snapshot.native.disconnected !== "data channel closed"
    ) throw new Error(`cleanup timeout: ${JSON.stringify(snapshot)}`);

    const result = {
      passed: true,
      browser_path: connectedSnapshot.browser.path,
      native_path: connectedSnapshot.native.path,
      browser_session: connectedSnapshot.browser.session_open,
      native_session: connectedSnapshot.native.session_open,
      browser_received_pong: connectedSnapshot.browser.pong_received,
      native_exchanged_packet: connectedSnapshot.native.packet_exchange,
      browser_disconnected: snapshot.browser.disconnected,
      native_disconnected: snapshot.native.disconnected,
    };
    output.textContent = JSON.stringify(result, null, 2);
    await post("/result", result);
  } catch (error) {
    const result = { passed: false, error: String(error) };
    output.textContent = JSON.stringify(result, null, 2);
    await post("/result", result);
  }
}

main();
</script>
"""


class Bridge:
    def __init__(self, native: Popen[str], assets: Path) -> None:
        self.native = native
        self.assets = assets
        self.lock = threading.Lock()
        self.native_stdin_lock = threading.Lock()
        self.native_signals: deque[dict[str, Any]] = deque()
        self.native_status: dict[str, Any] | None = None
        self.native_error: str | None = None
        self.result: dict[str, Any] | None = None
        self.result_event = threading.Event()

    def read_native(self) -> None:
        assert self.native.stdout is not None
        for line in self.native.stdout:
            try:
                message = json.loads(line)
            except json.JSONDecodeError as error:
                with self.lock:
                    self.native_error = f"invalid native harness JSON: {error}"
                continue
            with self.lock:
                if isinstance(message, dict) and isinstance(
                    message.get("harness"), dict
                ):
                    self.native_status = message["harness"]
                elif isinstance(message, dict):
                    self.native_signals.append(message)
                else:
                    self.native_error = "native harness emitted a non-object JSON value"

    def next_native_signal(self) -> dict[str, Any] | None:
        with self.lock:
            return self.native_signals.popleft() if self.native_signals else None

    def send_native_signal(self, signal: dict[str, Any]) -> None:
        if self.native.poll() is not None:
            raise RuntimeError("native harness exited before receiving the signal")
        assert self.native.stdin is not None
        with self.native_stdin_lock:
            self.native.stdin.write(json.dumps(signal) + "\n")
            self.native.stdin.flush()

    def snapshot(self) -> dict[str, Any]:
        with self.lock:
            return {
                "status": self.native_status,
                "error": self.native_error,
                "exited": self.native.poll() is not None,
            }

    def set_result(self, result: dict[str, Any]) -> None:
        with self.lock:
            self.result = result
        self.result_event.set()


class Handler(BaseHTTPRequestHandler):
    bridge: Bridge

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def send_json(self, value: object, status: HTTPStatus = HTTPStatus.OK) -> None:
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/":
            body = PAGE.encode()
            self.send_response(HTTPStatus.OK)
            self.send_header("content-type", "text/html")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path == "/favicon.ico":
            self.send_response(HTTPStatus.NO_CONTENT)
            self.end_headers()
        elif self.path == "/browser_wasm_harness.js":
            self.send_file("browser_wasm_harness.js", "text/javascript")
        elif self.path == "/browser_wasm_harness_bg.wasm":
            self.send_file("browser_wasm_harness_bg.wasm", "application/wasm")
        elif self.path == "/native/status":
            self.send_json(self.bridge.snapshot())
        elif self.path == "/native/signal":
            self.send_json(self.bridge.next_native_signal())
        else:
            self.send_error(HTTPStatus.NOT_FOUND)

    def send_file(self, name: str, content_type: str) -> None:
        path = self.bridge.assets / name
        try:
            body = path.read_bytes()
        except FileNotFoundError:
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        self.send_response(HTTPStatus.OK)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self) -> None:  # noqa: N802
        try:
            length = int(self.headers.get("content-length", "0"))
            value = json.loads(self.rfile.read(length))
            if self.path == "/native/signal":
                if not isinstance(value, dict):
                    raise ValueError("native signal must be a JSON object")
                self.bridge.send_native_signal(value)
            elif self.path == "/browser/status":
                pass
            elif self.path == "/result":
                if not isinstance(value, dict):
                    raise ValueError("harness result must be a JSON object")
                self.bridge.set_result(value)
            else:
                self.send_error(HTTPStatus.NOT_FOUND)
                return
            self.send_json({"ok": True})
        except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
            self.send_json({"error": str(error)}, HTTPStatus.INTERNAL_SERVER_ERROR)


def stop_native(native: Popen[str]) -> None:
    if native.poll() is not None:
        return
    if native.stdin is not None:
        try:
            native.stdin.close()
        except (BrokenPipeError, OSError, ValueError):
            pass
    try:
        native.wait(timeout=2)
    except subprocess.TimeoutExpired:
        native.terminate()
        try:
            native.wait(timeout=2)
        except subprocess.TimeoutExpired:
            native.kill()
            native.wait()


def browser_executable(configured: str | None) -> str:
    if configured is not None:
        return configured
    for name in ("chromium", "chromium-browser", "google-chrome"):
        if executable := shutil.which(name):
            return executable
    raise RuntimeError("Chromium is required to run the browser harness")


def stop_browser(browser: Popen[str]) -> None:
    if browser.poll() is not None:
        return
    browser.terminate()
    try:
        browser.wait(timeout=5)
    except subprocess.TimeoutExpired:
        browser.kill()
        browser.wait()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--browser")
    parser.add_argument("--timeout", type=float, default=90)
    args = parser.parse_args()

    environment = os.environ.copy()
    native: Popen[str] | None = None
    browser: Popen[str] | None = None
    browser_log = None
    with tempfile.TemporaryDirectory(prefix="aeronet-webrtc-") as directory:
        assets = Path(directory)
        try:
            build_harnesses(environment, assets)
            native = subprocess.Popen(
                [
                    str(
                        target_path(environment, ROOT / "target")
                        / NATIVE_EXAMPLE.relative_to(ROOT / "target")
                    )
                ],
                cwd=ROOT,
                env=environment,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                text=True,
                bufsize=1,
            )
            bridge = Bridge(native, assets)
            threading.Thread(target=bridge.read_native, daemon=True).start()

            Handler.bridge = bridge
            with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
                url = f"http://127.0.0.1:{server.server_port}/"
                print(f"BROWSER_HARNESS_URL={url}", flush=True)
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                try:
                    command = [
                        browser_executable(args.browser),
                        "--headless=new",
                        "--disable-dev-shm-usage",
                        "--disable-gpu",
                        "--no-default-browser-check",
                        "--no-first-run",
                        "--no-proxy-server",
                        f"--user-data-dir={assets / 'chromium-profile'}",
                        url,
                    ]
                    if hasattr(os, "geteuid") and os.geteuid() == 0:
                        command.insert(1, "--no-sandbox")
                    browser_log = (assets / "chromium.stderr").open("w")
                    browser = subprocess.Popen(
                        command,
                        cwd=ROOT,
                        env=environment,
                        stdout=subprocess.DEVNULL,
                        stderr=browser_log,
                        text=True,
                    )
                    deadline = time.monotonic() + args.timeout
                    while not bridge.result_event.wait(0.1):
                        if browser.poll() is not None:
                            browser_log.flush()
                            stderr = (assets / "chromium.stderr").read_text()
                            raise RuntimeError(
                                f"Chromium exited before reporting a result: {stderr.strip()}"
                            )
                        if time.monotonic() >= deadline:
                            raise TimeoutError("browser harness result timed out")
                finally:
                    server.shutdown()
                    thread.join()
            result = bridge.result
            if result is None or not result.get("passed"):
                print(
                    json.dumps(result or {"passed": False}, indent=2), file=sys.stderr
                )
                return 1
            native.wait(timeout=5)
            print(json.dumps(result, indent=2))
            return 0 if native.returncode == 0 else 1
        finally:
            if browser is not None:
                stop_browser(browser)
            if browser_log is not None:
                browser_log.close()
            if native is not None:
                stop_native(native)


if __name__ == "__main__":
    raise SystemExit(main())
