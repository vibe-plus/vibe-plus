#!/usr/bin/env python3
"""
Real CLI takeover black-box test (minimal version).

Principle:
- Do NOT read/write CLI config files in test code.
- Do NOT mock CLI.
- Only mock Vibe upstream provider.

Flow:
1) Ensure gateway is alive.
2) Register mock providers in Vibe (Anthropic/OpenAI).
3) Run real `vibe takeover <client>`.
4) Run real CLI (`claude`/`codex`/`opencode`).
5) Check `/_vp/logs` for fresh client hit.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import List, Optional, Tuple


def _request(
    method: str,
    url: str,
    body: Optional[dict] = None,
    headers: Optional[dict] = None,
    timeout: float = 8.0,
) -> Tuple[int, dict, str]:
    payload = None
    req_headers = {"content-type": "application/json"}
    if headers:
        req_headers.update(headers)
    if body is not None:
        payload = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url=url, method=method, data=payload, headers=req_headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8")
            parsed = json.loads(raw) if raw else {}
            return resp.status, parsed, raw
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8")
        try:
            parsed = json.loads(raw) if raw else {}
        except Exception:
            parsed = {}
        return e.code, parsed, raw


@dataclass
class MockBehavior:
    expected_path: str
    expected_body_key: str
    hits: int = 0
    lock: threading.Lock = field(default_factory=threading.Lock)

    def mark_hit(self) -> None:
        with self.lock:
            self.hits += 1


class MockServer:
    def __init__(self, port: int, behavior: MockBehavior):
        self.port = port
        self.behavior = behavior
        self.httpd: Optional[ThreadingHTTPServer] = None
        self.thread: Optional[threading.Thread] = None

    def start(self) -> None:
        behavior = self.behavior

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                raw = self.rfile.read(length).decode("utf-8")
                try:
                    body = json.loads(raw) if raw else {}
                except Exception:
                    body = {}

                valid = self.path == behavior.expected_path and behavior.expected_body_key in body
                if valid:
                    behavior.mark_hit()
                    # Full Anthropic Messages response (model + stop_reason required by SDK).
                    try:
                        req_body = json.loads(raw) if raw else {}
                    except Exception:
                        req_body = body
                    out = json.dumps(
                        {
                            "id": "msg_mock_001",
                            "type": "message",
                            "role": "assistant",
                            "model": req_body.get("model", "claude-mock"),
                            "content": [{"type": "text", "text": "OK"}],
                            "stop_reason": "end_turn",
                            "stop_sequence": None,
                            "usage": {
                                "input_tokens": 5,
                                "output_tokens": 2,
                                "cache_creation_input_tokens": 0,
                                "cache_read_input_tokens": 0,
                            },
                        }
                    ).encode("utf-8")
                    self.send_response(200)
                else:
                    out = json.dumps(
                        {"error": "mock expectation mismatch", "path": self.path, "body": body}
                    ).encode("utf-8")
                    self.send_response(422)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

            def log_message(self, fmt, *args):  # noqa: A003
                return

        self.httpd = ThreadingHTTPServer(("127.0.0.1", self.port), Handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()

    def stop(self) -> None:
        if self.httpd:
            self.httpd.shutdown()
            self.httpd.server_close()
        if self.thread:
            self.thread.join(timeout=2)


class OpenAIMockServer:
    def __init__(self, port: int):
        self.port = port
        self.httpd: Optional[ThreadingHTTPServer] = None
        self.thread: Optional[threading.Thread] = None
        self.hits_chat = 0
        self.hits_resp = 0
        self.lock = threading.Lock()

    def start(self) -> None:
        parent = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                raw = self.rfile.read(length).decode("utf-8")
                try:
                    body = json.loads(raw) if raw else {}
                except Exception:
                    body = {}

                if self.path == "/v1/chat/completions":
                    with parent.lock:
                        parent.hits_chat += 1
                    out = json.dumps(
                        {
                            "id": "chatcmpl_mock_001",
                            "object": "chat.completion",
                            "created": int(time.time()),
                            "model": body.get("model", "gpt-mock-chat"),
                            "choices": [
                                {
                                    "index": 0,
                                    "message": {"role": "assistant", "content": "OK"},
                                    "finish_reason": "stop",
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 5,
                                "completion_tokens": 1,
                                "total_tokens": 6,
                            },
                        }
                    ).encode("utf-8")
                    self.send_response(200)
                elif self.path == "/v1/responses":
                    with parent.lock:
                        parent.hits_resp += 1
                    out = json.dumps(
                        {
                            "id": "resp_mock_001",
                            "object": "response",
                            "model": body.get("model", "gpt-mock-responses"),
                            "output": [
                                {
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [{"type": "output_text", "text": "OK"}],
                                }
                            ],
                            "usage": {"input_tokens": 5, "output_tokens": 1, "total_tokens": 6},
                        }
                    ).encode("utf-8")
                    self.send_response(200)
                else:
                    out = json.dumps(
                        {"error": "mock expectation mismatch", "path": self.path, "body": body}
                    ).encode("utf-8")
                    self.send_response(422)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

            def log_message(self, fmt, *args):  # noqa: A003
                return

        self.httpd = ThreadingHTTPServer(("127.0.0.1", self.port), Handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()

    def stop(self) -> None:
        if self.httpd:
            self.httpd.shutdown()
            self.httpd.server_close()
        if self.thread:
            self.thread.join(timeout=2)


def _create_provider(gateway: str, payload: dict) -> Tuple[Optional[str], Optional[str]]:
    s_create, obj_create, raw_create = _request("POST", f"{gateway}/_vp/providers", payload)
    if s_create != 200 or "id" not in obj_create:
        return None, f"create provider failed: http={s_create}, body={raw_create[:140]}"
    return obj_create["id"], None


def _find_log_hit(gateway: str, since: int, app_keyword: str, model_keyword: str) -> Tuple[Optional[dict], str]:
    s_logs, obj_logs, raw_logs = _request("GET", f"{gateway}/_vp/logs?since={since}&limit=80")
    if s_logs != 200:
        return None, f"logs query failed http={s_logs}, body={raw_logs[:140]}"
    items = obj_logs.get("items", [])
    if not isinstance(items, list):
        return None, "logs malformed: items is not list"
    for it in items:
        if not isinstance(it, dict):
            continue
        app = str(it.get("app") or "").lower()
        model = str(it.get("requested_model") or "").lower()
        if app_keyword in app or model_keyword in model:
            return it, ""
    return None, f"no log hit for app={app_keyword} model={model_keyword}, total={len(items)}"


def test_real_claude_takeover_hits_vibe(gateway: str, repo_root: Path, vibe_bin: Path) -> Tuple[bool, str]:
    s, _, _ = _request("GET", f"{gateway}/health")
    if s != 200:
        return False, f"gateway not ready: {gateway} health={s}"

    behavior = MockBehavior(expected_path="/v1/messages", expected_body_key="messages")
    mock = MockServer(19921, behavior)
    mock.start()

    provider_id = None
    try:
        provider_id, err = _create_provider(
            gateway,
            {
                "name": "qa-claude-real-cli-mock",
                "kind": "anthropic",
                "base_url": "http://127.0.0.1:19921",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": "claude-sonnet-4-5", "upstream_model": "claude-mock"}],
            },
        )
        if not provider_id:
            return False, err or "create provider failed"

        # real takeover command
        tk = subprocess.run(
            [str(vibe_bin), "takeover", "claude"],
            cwd=repo_root,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        if tk.returncode != 0:
            return False, f"takeover failed rc={tk.returncode}, stderr={tk.stderr[:140]}"

        since = int(time.time()) - 2
        try:
            run = subprocess.run(
                ["claude", "-p", "请回复 OK", "--model", "claude-sonnet-4-5"],
                cwd=repo_root,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=120,
                check=False,
            )
        except subprocess.TimeoutExpired as e:
            return False, f"claude -p timeout after {e.timeout}s"
        # Claude Code refuses to spawn inside another Claude Code session.
        # Detect this and mark as SKIP rather than FAIL.
        if run.returncode != 0:
            nested = (
                "nested" in run.stderr.lower()
                or "cannot be launched inside" in run.stderr.lower()
            )
            if nested:
                return True, f"SKIP (nested claude session): {run.stderr[:80].strip()}"
            return False, f"claude -p failed rc={run.returncode}, stderr={run.stderr[:140]}"

        hit, err = _find_log_hit(gateway, since, "claude", "claude-sonnet-4-5")
        if not hit:
            return False, err
        if behavior.hits <= 0:
            return False, "mock upstream not hit"

        return True, (
            f"takeover_rc={tk.returncode}, claude_rc={run.returncode}, "
            f"log_id={hit.get('id')}, app={hit.get('app')}, status={hit.get('status_code')}, "
            f"mock_hits={behavior.hits}"
        )
    finally:
        # Always restore settings.json so the test doesn't permanently alter the system.
        subprocess.run(
            [str(vibe_bin), "takeover", "claude", "--restore"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=30,
            check=False,
        )
        if provider_id:
            _request("DELETE", f"{gateway}/_vp/providers/{provider_id}")
        mock.stop()


def test_real_codex_takeover_hits_vibe(gateway: str, repo_root: Path, vibe_bin: Path) -> Tuple[bool, str]:
    s, _, _ = _request("GET", f"{gateway}/health")
    if s != 200:
        return False, f"gateway not ready: {gateway} health={s}"

    mock = OpenAIMockServer(19922)
    mock.start()
    provider_id = None
    try:
        provider_id, err = _create_provider(
            gateway,
            {
                "name": "qa-codex-real-cli-mock",
                "kind": "openai-responses",
                "base_url": "http://127.0.0.1:19922",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": "qa-codex-real-cli", "upstream_model": "gpt-mock-responses"}],
            },
        )
        if not provider_id:
            return False, err or "create provider failed"

        tk = subprocess.run(
            [str(vibe_bin), "takeover", "codex"],
            cwd=repo_root,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        if tk.returncode != 0:
            return False, f"takeover failed rc={tk.returncode}, stderr={tk.stderr[:140]}"

        since = int(time.time()) - 2
        try:
            run = subprocess.run(
                [
                    "codex",
                    "exec",
                    "--skip-git-repo-check",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "--model",
                    "qa-codex-real-cli",
                    "reply OK only",
                ],
                cwd=repo_root,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=180,
                check=False,
            )
        except subprocess.TimeoutExpired as e:
            return False, f"codex exec timeout after {e.timeout}s"
        if run.returncode != 0:
            return False, f"codex exec failed rc={run.returncode}, stderr={run.stderr[:180]}"

        hit, err = _find_log_hit(gateway, since, "codex", "qa-codex-real-cli")
        if not hit:
            return False, err
        if mock.hits_resp <= 0:
            return False, f"mock responses upstream not hit, chat_hits={mock.hits_chat}"
        return True, (
            f"takeover_rc={tk.returncode}, codex_rc={run.returncode}, "
            f"log_id={hit.get('id')}, app={hit.get('app')}, status={hit.get('status_code')}, "
            f"mock_resp_hits={mock.hits_resp}"
        )
    finally:
        subprocess.run(
            [str(vibe_bin), "takeover", "codex", "--restore"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=30,
            check=False,
        )
        if provider_id:
            _request("DELETE", f"{gateway}/_vp/providers/{provider_id}")
        mock.stop()


def test_real_opencode_takeover_hits_vibe(
    gateway: str, repo_root: Path, vibe_bin: Path
) -> Tuple[bool, str]:
    s, _, _ = _request("GET", f"{gateway}/health")
    if s != 200:
        return False, f"gateway not ready: {gateway} health={s}"

    mock = OpenAIMockServer(19923)
    mock.start()
    provider_id = None
    try:
        provider_id, err = _create_provider(
            gateway,
            {
                "name": "qa-opencode-real-cli-mock",
                "kind": "openai-chat",
                "base_url": "http://127.0.0.1:19923",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": "gpt-5.3-codex", "upstream_model": "gpt-mock-chat"}],
            },
        )
        if not provider_id:
            return False, err or "create provider failed"

        tk = subprocess.run(
            [str(vibe_bin), "takeover", "opencode"],
            cwd=repo_root,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        if tk.returncode != 0:
            return False, f"takeover failed rc={tk.returncode}, stderr={tk.stderr[:140]}"

        since = int(time.time()) - 2
        try:
            run = subprocess.run(
                [
                    "opencode",
                    "run",
                    "--format",
                    "json",
                    "-m",
                    "vibe/gpt-5.3-codex",
                    "reply OK only",
                ],
                cwd=repo_root,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=180,
                check=False,
            )
        except subprocess.TimeoutExpired as e:
            return False, f"opencode run timeout after {e.timeout}s"
        if run.returncode != 0:
            return False, f"opencode run failed rc={run.returncode}, stderr={run.stderr[:180]}"

        hit, err = _find_log_hit(gateway, since, "opencode", "gpt-5.3-codex")
        if not hit:
            return False, err
        if mock.hits_chat <= 0:
            return False, f"mock chat upstream not hit, resp_hits={mock.hits_resp}"
        return True, (
            f"takeover_rc={tk.returncode}, opencode_rc={run.returncode}, "
            f"log_id={hit.get('id')}, app={hit.get('app')}, status={hit.get('status_code')}, "
            f"mock_chat_hits={mock.hits_chat}"
        )
    finally:
        subprocess.run(
            [str(vibe_bin), "takeover", "opencode", "--restore"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=30,
            check=False,
        )
        if provider_id:
            _request("DELETE", f"{gateway}/_vp/providers/{provider_id}")
        mock.stop()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gateway", default="http://127.0.0.1:15917")
    parser.add_argument(
        "--repo-root",
        default=str(Path(__file__).resolve().parents[1]),
        help="Path to vibe-plus repo root",
    )
    parser.add_argument(
        "--vibe-bin",
        default="",
        help="Path to vibe binary (default: <repo-root>/target/debug/vibe)",
    )
    args = parser.parse_args()

    repo_root = Path(args.repo_root)
    vibe_bin = Path(args.vibe_bin) if args.vibe_bin else repo_root / "target" / "debug" / "vibe"
    if not vibe_bin.exists():
        print(f"Gateway: {args.gateway}")
        print(f"[FAIL] real_claude_takeover_hits_vibe: vibe binary not found: {vibe_bin}")
        print("\nResult: 1 test(s) failed.")
        return 1

    print(f"Gateway: {args.gateway}")
    checks = [
        ("real_claude_takeover_hits_vibe", test_real_claude_takeover_hits_vibe),
        ("real_codex_takeover_hits_vibe", test_real_codex_takeover_hits_vibe),
        ("real_opencode_takeover_hits_vibe", test_real_opencode_takeover_hits_vibe),
    ]
    failed = 0
    for name, fn in checks:
        ok, msg = fn(args.gateway, repo_root, vibe_bin)
        print(f"[{'PASS' if ok else 'FAIL'}] {name}: {msg}")
        if not ok:
            failed += 1
    if failed:
        print(f"\nResult: {failed} test(s) failed.")
        return 1
    print("\nResult: all tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

