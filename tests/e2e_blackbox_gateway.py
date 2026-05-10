#!/usr/bin/env python3
"""
Black-box E2E tests for vibe gateway.

Design goals:
- Treat gateway as a black box (HTTP only).
- Do not depend on project-internal unit/integration tests.
- Include both "currently expected to pass" and "product expectation" checks.

Usage:
  python3 tests/e2e_blackbox_gateway.py --gateway http://127.0.0.1:15917
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import random
import string
import tempfile
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Dict, List, Optional, Tuple


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


def _rid(prefix: str) -> str:
    s = "".join(random.choice(string.ascii_lowercase + string.digits) for _ in range(6))
    return f"{prefix}-{s}"


@dataclass
class MockBehavior:
    status: int
    content: str
    model: str
    sleep_ms: int = 0
    hits: int = 0
    paths: List[str] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def mark_hit(self, path: str) -> None:
        with self.lock:
            self.hits += 1
            self.paths.append(path)


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
                _ = self.rfile.read(length)
                behavior.mark_hit(self.path)
                if behavior.sleep_ms > 0:
                    time.sleep(behavior.sleep_ms / 1000.0)
                body = {
                    "id": f"mock-{behavior.model}",
                    "object": "chat.completion",
                    "created": int(time.time()),
                    "model": behavior.model,
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": behavior.content},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                }
                raw = json.dumps(body).encode("utf-8")
                self.send_response(behavior.status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

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


class GatewayClient:
    def __init__(self, base: str):
        self.base = base.rstrip("/")

    def create_provider(
        self,
        name: str,
        base_url: str,
        alias: str,
        upstream_model: str,
        priority: int,
    ) -> str:
        status, obj, raw = _request(
            "POST",
            f"{self.base}/_vp/providers",
            {
                "name": name,
                "kind": "openai-chat",
                "base_url": base_url,
                "auth_ref": None,
                "enabled": True,
                "priority": priority,
                "model_aliases": [{"alias": alias, "upstream_model": upstream_model}],
            },
        )
        if status != 200 or "id" not in obj:
            raise RuntimeError(f"create provider failed: status={status}, body={raw}")
        return obj["id"]

    def delete_provider(self, pid: str) -> None:
        _request("DELETE", f"{self.base}/_vp/providers/{pid}")

    def chat(self, model: str, ua: Optional[str] = None) -> Tuple[int, dict, str]:
        headers = {"user-agent": ua} if ua else None
        return _request(
            "POST",
            f"{self.base}/v1/chat/completions",
            {"model": model, "messages": [{"role": "user", "content": "ping"}]},
            headers=headers,
        )

    def post_json(self, path: str, body: dict, ua: Optional[str] = None) -> Tuple[int, dict, str]:
        headers = {"user-agent": ua} if ua else None
        return _request("POST", f"{self.base}{path}", body, headers=headers)

    def chat_at(self, path: str, model: str, ua: Optional[str] = None) -> Tuple[int, dict, str]:
        headers = {"user-agent": ua} if ua else None
        return _request(
            "POST",
            f"{self.base}{path}",
            {"model": model, "messages": [{"role": "user", "content": "ping"}]},
            headers=headers,
        )


def test_retry_and_failover(client: GatewayClient) -> Tuple[bool, str]:
    alias = _rid("qa-retry")
    b1 = MockBehavior(status=500, content="primary-500", model="m-primary")
    b2 = MockBehavior(status=200, content="fallback-200", model="m-fallback")
    s1 = MockServer(19401, b1)
    s2 = MockServer(19402, b2)
    s1.start()
    s2.start()

    p1 = p2 = None
    try:
        p1 = client.create_provider(_rid("e2e-p1"), "http://127.0.0.1:19401", alias, "m", 1)
        p2 = client.create_provider(_rid("e2e-p2"), "http://127.0.0.1:19402", alias, "m", 2)
        status, obj, raw = client.chat(alias)
        ok = status == 200 and obj.get("choices", [{}])[0].get("message", {}).get("content") == "fallback-200"
        msg = f"status={status}, content={obj.get('choices',[{}])[0].get('message',{}).get('content')}, hits500={b1.hits}, hits200={b2.hits}"
        return ok, msg if ok else f"{msg}, raw={raw}"
    finally:
        if p1:
            client.delete_provider(p1)
        if p2:
            client.delete_provider(p2)
        s1.stop()
        s2.stop()


def test_session_affinity_expectation(client: GatewayClient) -> Tuple[bool, str]:
    """
    Product expectation check:
    Two same-priority providers should both receive traffic under concurrency,
    and session-level routing should not collapse to one provider globally.
    """
    alias = _rid("qa-session")
    b1 = MockBehavior(status=200, content="from-A", model="m-a")
    b2 = MockBehavior(status=200, content="from-B", model="m-b")
    s1 = MockServer(19411, b1)
    s2 = MockServer(19412, b2)
    s1.start()
    s2.start()

    p1 = p2 = None
    try:
        p1 = client.create_provider(_rid("e2e-sa"), "http://127.0.0.1:19411", alias, "m", 10)
        p2 = client.create_provider(_rid("e2e-sb"), "http://127.0.0.1:19412", alias, "m", 10)

        def one(i: int) -> Tuple[str, str]:
            ua = "sess-A" if i % 2 == 0 else "sess-B"
            status, obj, _ = client.chat(alias, ua=ua)
            content = obj.get("choices", [{}])[0].get("message", {}).get("content", "")
            if status != 200:
                return ua, f"HTTP-{status}"
            return ua, content

        results: List[Tuple[str, str]] = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=20) as ex:
            for r in ex.map(one, range(60)):
                results.append(r)

        by_session: Dict[str, Dict[str, int]] = {"sess-A": {}, "sess-B": {}}
        for ua, content in results:
            by_session[ua][content] = by_session[ua].get(content, 0) + 1

        # Expected (product-level) signal: both providers should be used.
        both_used = b1.hits > 0 and b2.hits > 0
        ok = both_used
        msg = (
            f"hitsA={b1.hits}, hitsB={b2.hits}, "
            f"sessA={by_session['sess-A']}, sessB={by_session['sess-B']}"
        )
        return ok, msg
    finally:
        if p1:
            client.delete_provider(p1)
        if p2:
            client.delete_provider(p2)
        s1.stop()
        s2.stop()


def test_error_code_failover_matrix(client: GatewayClient) -> Tuple[bool, str]:
    """
    Validate current failover policy by status code:
    - 401/402/429/5xx -> should failover
    - 400 -> should NOT failover
    """
    cases = [
        (401, True),
        (402, True),
        (400, False),
        (429, True),
        (500, True),
    ]
    all_ok = True
    details: List[str] = []

    for code, expect_failover in cases:
        alias = _rid(f"qa-code-{code}")
        primary = MockBehavior(status=code, content=f"primary-{code}", model=f"m-{code}-p")
        fallback = MockBehavior(status=200, content=f"fallback-{code}", model=f"m-{code}-f")
        s1 = MockServer(19500 + code, primary)
        s2 = MockServer(19600 + code, fallback)
        s1.start()
        s2.start()
        p1 = p2 = None
        try:
            p1 = client.create_provider(_rid("e2e-cp"), f"http://127.0.0.1:{19500 + code}", alias, "m", 1)
            p2 = client.create_provider(_rid("e2e-cf"), f"http://127.0.0.1:{19600 + code}", alias, "m", 2)
            status, obj, _ = client.chat(alias)
            got_fallback = (
                status == 200
                and obj.get("choices", [{}])[0].get("message", {}).get("content") == f"fallback-{code}"
            )
            ok = got_fallback == expect_failover
            all_ok = all_ok and ok
            details.append(
                f"{code}: expected_failover={expect_failover}, got_failover={got_fallback}, "
                f"http={status}, hits_primary={primary.hits}, hits_fallback={fallback.hits}"
            )
        finally:
            if p1:
                client.delete_provider(p1)
            if p2:
                client.delete_provider(p2)
            s1.stop()
            s2.stop()

    return all_ok, " | ".join(details)


def test_payment_and_auth_semantics_expectation(client: GatewayClient) -> Tuple[bool, str]:
    """
    Product expectation test (likely failing on current implementation):
    - 402 (payment issue) should temporarily pause provider and use fallback.
    - 401 (auth invalid) should permanently disable provider and use fallback.
    Current code does not distinguish these semantics.
    """
    alias_402 = _rid("qa-exp-402")
    alias_401 = _rid("qa-exp-401")

    b402 = MockBehavior(status=402, content="need-payment", model="m402")
    b401 = MockBehavior(status=401, content="bad-key", model="m401")
    bfb = MockBehavior(status=200, content="fallback-ok", model="mfb")

    s402 = MockServer(19702, b402)
    s401 = MockServer(19701, b401)
    sfb = MockServer(19700, bfb)
    s402.start()
    s401.start()
    sfb.start()

    p402 = p401 = pfb1 = pfb2 = None
    try:
        p402 = client.create_provider(_rid("e2e-402"), "http://127.0.0.1:19702", alias_402, "m", 1)
        pfb1 = client.create_provider(_rid("e2e-fb1"), "http://127.0.0.1:19700", alias_402, "m", 2)
        p401 = client.create_provider(_rid("e2e-401"), "http://127.0.0.1:19701", alias_401, "m", 1)
        pfb2 = client.create_provider(_rid("e2e-fb2"), "http://127.0.0.1:19700", alias_401, "m", 2)

        # Probe multiple times to simulate "pause/disable then fallback".
        result_402 = [client.chat(alias_402)[0] for _ in range(3)]
        result_401 = [client.chat(alias_401)[0] for _ in range(3)]

        # Product expectation: after first failure, follow-up should be 200 via fallback.
        expect_402 = result_402[1:] == [200, 200]
        expect_401 = result_401[1:] == [200, 200]
        ok = expect_402 and expect_401
        msg = (
            f"402_statuses={result_402}, 401_statuses={result_401}, "
            f"hits402={b402.hits}, hits401={b401.hits}, hitsFallback={bfb.hits}"
        )
        return ok, msg
    finally:
        for pid in [p402, p401, pfb1, pfb2]:
            if pid:
                client.delete_provider(pid)
        s402.stop()
        s401.stop()
        sfb.stop()


def test_tool_path_routing_expectation(client: GatewayClient) -> Tuple[bool, str]:
    """
    Product expectation test:
    tool-specific routes should be consistently available.
    """
    checks: List[Tuple[str, int, str]] = []
    ok = True

    s1, _o1, r1 = client.chat_at("/codex/v1/chat/completions", "openai-lb")
    checks.append(("codex", s1, r1[:80]))
    ok = ok and s1 != 404

    s2, _o2, r2 = client.chat_at("/opencode/v1/chat/completions", "openai-lb")
    checks.append(("opencode", s2, r2[:80]))
    ok = ok and s2 != 404

    s3, _o3, r3 = client.post_json(
        "/claude/v1/messages",
        {"model": "anth-lb", "max_tokens": 16, "messages": [{"role": "user", "content": "ping"}]},
    )
    checks.append(("claude", s3, r3[:80]))
    ok = ok and s3 != 404

    msg = " | ".join([f"{name}:http={code},body={body}" for name, code, body in checks])
    return ok, msg


def test_gemini_native_route_expectation(client: GatewayClient) -> Tuple[bool, str]:
    """
    Product expectation test:
    Gemini native route should preserve Gemini protocol semantics.
    """
    alias = _rid("qa-gem-sem")
    received: Dict[str, str] = {"path": "", "body": ""}

    class GeminiHandler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length).decode("utf-8")
            received["path"] = self.path
            received["body"] = raw
            try:
                body = json.loads(raw) if raw else {}
            except Exception:
                body = {}

            ok_req = (
                self.path == "/v1beta/models/gemini-2.5-pro:generateContent"
                and isinstance(body.get("contents"), list)
                and len(body.get("contents", [])) > 0
            )

            if ok_req:
                out = {
                    "candidates": [
                        {
                            "content": {"role": "model", "parts": [{"text": "gemini-ok"}]},
                            "finishReason": "STOP",
                        }
                    ],
                    "usageMetadata": {
                        "promptTokenCount": 3,
                        "candidatesTokenCount": 5,
                        "totalTokenCount": 8,
                    },
                }
                raw_out = json.dumps(out).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw_out)))
                self.end_headers()
                self.wfile.write(raw_out)
            else:
                raw_out = b'{"error":"not gemini native protocol"}'
                self.send_response(422)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw_out)))
                self.end_headers()
                self.wfile.write(raw_out)

        def log_message(self, fmt, *args):  # noqa: A003
            return

    class ReuseTCPServer(ThreadingHTTPServer):
        allow_reuse_address = True

    server = ReuseTCPServer(("127.0.0.1", 19830), GeminiHandler)
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()

    pid = None
    try:
        s, obj, raw = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-gem-sem"),
                "kind": "gemini-native",
                "base_url": "http://127.0.0.1:19830",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": alias, "upstream_model": "gemini-2.5-pro"}],
            },
        )
        if s != 200 or "id" not in obj:
            raise RuntimeError(f"create provider failed: {s} {raw}")
        pid = obj["id"]

        status, _obj, body = client.post_json(
            "/v1beta/models/gemini-2.5-pro:generateContent",
            {"contents": [{"role": "user", "parts": [{"text": "ping"}]}]},
        )
        ok = status == 200 and "/v1beta/models/gemini-2.5-pro:generateContent" in received["path"]
        msg = f"http={status}, upstream_path={received['path']}, upstream_body={received['body'][:100]}, raw={body[:100]}"
        return ok, msg
    finally:
        if pid:
            client.delete_provider(pid)
        server.shutdown()
        server.server_close()
        t.join(timeout=2)


def test_responses_protocol_semantics_expectation(client: GatewayClient) -> Tuple[bool, str]:
    """
    Product expectation test:
    /v1/responses should forward as Responses protocol, not Chat Completions protocol.
    """
    alias = _rid("qa-resp-sem")
    received: Dict[str, str] = {"path": "", "body": ""}

    class RespHandler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length).decode("utf-8")
            received["path"] = self.path
            received["body"] = raw
            # Strict expectation: upstream should receive /v1/responses payload with "input".
            try:
                body = json.loads(raw) if raw else {}
            except Exception:
                body = {}
            if self.path == "/v1/responses" and "input" in body:
                out = {
                    "id": "resp_123",
                    "object": "response",
                    "model": "gpt-mock",
                    "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "ok"}]}],
                }
                raw_out = json.dumps(out).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw_out)))
                self.end_headers()
                self.wfile.write(raw_out)
            else:
                raw_out = b'{"error":"not responses protocol"}'
                self.send_response(422)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw_out)))
                self.end_headers()
                self.wfile.write(raw_out)

        def log_message(self, fmt, *args):  # noqa: A003
            return

    class ReuseTCPServer(ThreadingHTTPServer):
        allow_reuse_address = True

    server = ReuseTCPServer(("127.0.0.1", 19820), RespHandler)
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()

    pid = None
    try:
        s, obj, raw = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-resp-sem"),
                "kind": "openai-responses",
                "base_url": "http://127.0.0.1:19820",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": alias, "upstream_model": "gpt-mock"}],
            },
        )
        if s != 200 or "id" not in obj:
            raise RuntimeError(f"create provider failed: {s} {raw}")
        pid = obj["id"]

        status, _obj, body = client.post_json(
            "/v1/responses",
            {"model": alias, "input": "ping"},
        )
        ok = status == 200
        msg = f"http={status}, upstream_path={received['path']}, upstream_body={received['body'][:100]}, raw={body[:100]}"
        return ok, msg
    finally:
        if pid:
            client.delete_provider(pid)
        server.shutdown()
        server.server_close()
        t.join(timeout=2)


def test_protocol_routes_and_forwarding(client: GatewayClient) -> Tuple[bool, str]:
    """
    Validate protocol entry routes are parseable and forwarded successfully.
    Also verify the upstream path actually used by gateway.
    """
    anth_alias = _rid("qa-anth")
    chat_alias = _rid("qa-chat")
    resp_alias = _rid("qa-resp")

    # Anthropic-shaped upstream response.
    anth = MockBehavior(status=200, content="anth-ok", model="claude-mock")
    # OpenAI-shaped upstream response.
    openai = MockBehavior(status=200, content="chat-ok", model="gpt-mock")

    s_anth = MockServer(19801, anth)
    s_openai = MockServer(19802, openai)
    s_anth.start()
    s_openai.start()

    p_anth = p_chat = p_resp = None
    try:
        # create_provider helper uses openai-chat; use raw API for kind-specific creation.
        p_anth_status, p_anth_obj, p_anth_raw = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-anth"),
                "kind": "anthropic",
                "base_url": "http://127.0.0.1:19801",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": anth_alias, "upstream_model": "claude-mock"}],
            },
        )
        if p_anth_status != 200 or "id" not in p_anth_obj:
            raise RuntimeError(f"create anthropic failed: {p_anth_status} {p_anth_raw}")
        p_anth = p_anth_obj["id"]

        p_chat_status, p_chat_obj, p_chat_raw = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-chat"),
                "kind": "openai-chat",
                "base_url": "http://127.0.0.1:19802",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": chat_alias, "upstream_model": "gpt-mock"}],
            },
        )
        if p_chat_status != 200 or "id" not in p_chat_obj:
            raise RuntimeError(f"create openai-chat failed: {p_chat_status} {p_chat_raw}")
        p_chat = p_chat_obj["id"]

        p_resp_status, p_resp_obj, p_resp_raw = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-resp"),
                "kind": "openai-responses",
                "base_url": "http://127.0.0.1:19802",
                "auth_ref": None,
                "enabled": True,
                "priority": 1,
                "model_aliases": [{"alias": resp_alias, "upstream_model": "gpt-mock"}],
            },
        )
        if p_resp_status != 200 or "id" not in p_resp_obj:
            raise RuntimeError(f"create openai-responses failed: {p_resp_status} {p_resp_raw}")
        p_resp = p_resp_obj["id"]

        # /v1/messages -> anthropic wire.
        s1, o1, _ = client.post_json(
            "/v1/messages",
            {
                "model": anth_alias,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}],
            },
        )
        # /v1/chat/completions -> openai chat wire.
        s2, o2, _ = client.post_json(
            "/v1/chat/completions",
            {"model": chat_alias, "messages": [{"role": "user", "content": "hi"}]},
        )
        # /v1/responses -> OpenAI Responses wire.
        s3, o3, _ = client.post_json(
            "/v1/responses",
            {"model": resp_alias, "input": "hi"},
        )

        ok_status = (s1, s2, s3) == (200, 200, 200)
        ok_paths = (
            any("/v1/messages" in p for p in anth.paths)
            and any("/v1/chat/completions" in p for p in openai.paths)
        )
        ok_resp_behavior = any("/v1/responses" in p for p in openai.paths)
        ok = ok_status and ok_paths and ok_resp_behavior
        msg = (
            f"http=({s1},{s2},{s3}), "
            f"anth_paths={anth.paths[-3:]}, openai_paths={openai.paths[-5:]}, "
            f"models=({o1.get('model')},{o2.get('model')},{o3.get('model')})"
        )
        return ok, msg
    finally:
        for pid in [p_anth, p_chat, p_resp]:
            if pid:
                client.delete_provider(pid)
        s_anth.stop()
        s_openai.stop()


def test_multi_auth_switching_provider_scoped(client: GatewayClient) -> Tuple[bool, str]:
    """
    Validate provider-scoped auth isolation:
    - Claude route must use Anthropic-style `x-api-key`.
    - Codex/OpenAI Responses route must use OpenAI-style `Authorization: Bearer`.
    - Two providers with different auth refs can coexist and be switched by model/route.
    """
    claude_alias = _rid("qa-claude-auth")
    codex_alias = _rid("qa-codex-auth")
    expected_claude_key = "claude-secret-key"
    expected_codex_key = "codex-secret-key"
    received: Dict[str, List[str]] = {
        "claude_x_api_key": [],
        "claude_auth": [],
        "codex_x_api_key": [],
        "codex_auth": [],
    }

    class ClaudeAuthHandler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            length = int(self.headers.get("Content-Length", "0"))
            _ = self.rfile.read(length)
            received["claude_x_api_key"].append(self.headers.get("x-api-key", ""))
            received["claude_auth"].append(self.headers.get("Authorization", ""))
            if self.path == "/v1/messages" and self.headers.get("x-api-key") == expected_claude_key:
                out = {
                    "id": "msg_auth_ok",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "claude-auth-ok"}],
                }
                raw_out = json.dumps(out).encode("utf-8")
                self.send_response(200)
            else:
                raw_out = b'{"error":"bad anthropic auth"}'
                self.send_response(401)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw_out)))
            self.end_headers()
            self.wfile.write(raw_out)

        def log_message(self, fmt, *args):  # noqa: A003
            return

    class CodexAuthHandler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            length = int(self.headers.get("Content-Length", "0"))
            _ = self.rfile.read(length)
            received["codex_x_api_key"].append(self.headers.get("x-api-key", ""))
            received["codex_auth"].append(self.headers.get("Authorization", ""))
            if self.path == "/v1/responses" and self.headers.get("Authorization") == f"Bearer {expected_codex_key}":
                out = {
                    "id": "resp_auth_ok",
                    "object": "response",
                    "model": "gpt-auth-ok",
                    "output": [
                        {
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": "codex-auth-ok"}],
                        }
                    ],
                }
                raw_out = json.dumps(out).encode("utf-8")
                self.send_response(200)
            else:
                raw_out = b'{"error":"bad openai auth"}'
                self.send_response(401)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw_out)))
            self.end_headers()
            self.wfile.write(raw_out)

        def log_message(self, fmt, *args):  # noqa: A003
            return

    class ReuseTCPServer(ThreadingHTTPServer):
        allow_reuse_address = True

    claude_server = ReuseTCPServer(("127.0.0.1", 19841), ClaudeAuthHandler)
    codex_server = ReuseTCPServer(("127.0.0.1", 19842), CodexAuthHandler)
    claude_thread = threading.Thread(target=claude_server.serve_forever, daemon=True)
    codex_thread = threading.Thread(target=codex_server.serve_forever, daemon=True)
    claude_thread.start()
    codex_thread.start()

    p_claude = p_codex = None
    try:
        s1, o1, r1 = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-claude-auth"),
                "kind": "anthropic",
                "base_url": "http://127.0.0.1:19841",
                "auth_ref": f"literal:{expected_claude_key}",
                "enabled": True,
                "priority": -1000,
                "model_aliases": [{"alias": claude_alias, "upstream_model": "claude-mock"}],
            },
        )
        if s1 != 200 or "id" not in o1:
            raise RuntimeError(f"create claude auth provider failed: {s1} {r1}")
        p_claude = o1["id"]

        s2, o2, r2 = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-codex-auth"),
                "kind": "openai-responses",
                "base_url": "http://127.0.0.1:19842",
                "auth_ref": f"literal:{expected_codex_key}",
                "enabled": True,
                "priority": -1000,
                "model_aliases": [{"alias": codex_alias, "upstream_model": "gpt-mock"}],
            },
        )
        if s2 != 200 or "id" not in o2:
            raise RuntimeError(f"create codex auth provider failed: {s2} {r2}")
        p_codex = o2["id"]

        c_status, c_obj, _ = client.post_json(
            "/v1/messages",
            {
                "model": claude_alias,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "ping"}],
            },
        )
        x_status, x_obj, _ = client.post_json(
            "/v1/responses",
            {"model": codex_alias, "input": "ping"},
        )

        claude_ok = c_status == 200 and c_obj.get("id") == "msg_auth_ok"
        codex_ok = x_status == 200 and x_obj.get("id") == "resp_auth_ok"
        no_cross = (
            all(v == expected_claude_key for v in received["claude_x_api_key"] if v)
            and all(v == "" for v in received["claude_auth"])
            and all(v == "" for v in received["codex_x_api_key"])
            and all(v == f"Bearer {expected_codex_key}" for v in received["codex_auth"] if v)
        )
        ok = claude_ok and codex_ok and no_cross
        msg = (
            f"claude_http={c_status}, codex_http={x_status}, "
            f"claude_x_api_key={received['claude_x_api_key']}, claude_auth={received['claude_auth']}, "
            f"codex_x_api_key={received['codex_x_api_key']}, codex_auth={received['codex_auth']}"
        )
        return ok, msg
    finally:
        if p_claude:
            client.delete_provider(p_claude)
        if p_codex:
            client.delete_provider(p_codex)
        claude_server.shutdown()
        codex_server.shutdown()
        claude_server.server_close()
        codex_server.server_close()
        claude_thread.join(timeout=2)
        codex_thread.join(timeout=2)


def test_auth_json_ref_not_supported_yet(client: GatewayClient) -> Tuple[bool, str]:
    """
    Current behavior check:
    vibe only resolves auth_ref via keyring/env/literal schemes.
    `file:/.../auth.json` (Codex-style local auth file) is currently unsupported.
    """
    alias = _rid("qa-auth-json")
    mock = MockBehavior(status=200, content="should-not-hit", model="m-auth-json")
    server = MockServer(19852, mock)
    server.start()
    pid = None
    try:
        with tempfile.TemporaryDirectory(prefix="vibe-auth-json-") as td:
            auth_path = f"{td}/auth.json"
            with open(auth_path, "w", encoding="utf-8") as f:
                json.dump(
                    {
                        "tokens": {"id_token": "dummy.jwt.token"},
                        "provider": "codex",
                    },
                    f,
                )

            status, obj, raw = _request(
                "POST",
                f"{client.base}/_vp/providers",
                {
                    "name": _rid("e2e-auth-json"),
                    "kind": "openai-responses",
                    "base_url": "http://127.0.0.1:19852",
                    "auth_ref": f"file:{auth_path}",
                    "enabled": True,
                    "priority": -1000,
                    "model_aliases": [{"alias": alias, "upstream_model": "gpt-mock"}],
                },
            )
            if status != 200 or "id" not in obj:
                raise RuntimeError(f"create provider failed: {status} {raw}")
            pid = obj["id"]

            req_status, _req_obj, req_raw = client.post_json(
                "/v1/responses",
                {"model": alias, "input": "ping"},
            )
            unsupported = req_status == 503 and "unknown auth_ref scheme" in req_raw
            ok = unsupported and mock.hits == 0
            msg = f"http={req_status}, body={req_raw[:120]}, upstream_hits={mock.hits}"
            return ok, msg
    finally:
        if pid:
            client.delete_provider(pid)
        server.stop()


def test_auth_header_mode_by_provider_kind_not_secret_pattern(client: GatewayClient) -> Tuple[bool, str]:
    """
    Behavior check:
    auth header mode is selected by provider kind/route, not by secret string pattern.
    """
    claude_alias = _rid("qa-kind-claude")
    codex_alias = _rid("qa-kind-codex")
    anthropic_like_openai = "Bearer sk-openai-style-token"
    openai_like_anthropic = "sk-ant-style-token"
    received: Dict[str, List[str]] = {
        "claude_x_api_key": [],
        "claude_auth": [],
        "codex_x_api_key": [],
        "codex_auth": [],
    }

    class ClaudeKindHandler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            length = int(self.headers.get("Content-Length", "0"))
            _ = self.rfile.read(length)
            received["claude_x_api_key"].append(self.headers.get("x-api-key", ""))
            received["claude_auth"].append(self.headers.get("Authorization", ""))
            raw_out = b'{"id":"msg_kind_ok","type":"message","role":"assistant","content":[{"type":"text","text":"ok"}]}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw_out)))
            self.end_headers()
            self.wfile.write(raw_out)

        def log_message(self, fmt, *args):  # noqa: A003
            return

    class CodexKindHandler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            length = int(self.headers.get("Content-Length", "0"))
            _ = self.rfile.read(length)
            received["codex_x_api_key"].append(self.headers.get("x-api-key", ""))
            received["codex_auth"].append(self.headers.get("Authorization", ""))
            raw_out = b'{"id":"resp_kind_ok","object":"response","model":"gpt-kind-ok","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw_out)))
            self.end_headers()
            self.wfile.write(raw_out)

        def log_message(self, fmt, *args):  # noqa: A003
            return

    class ReuseTCPServer(ThreadingHTTPServer):
        allow_reuse_address = True

    claude_server = ReuseTCPServer(("127.0.0.1", 19853), ClaudeKindHandler)
    codex_server = ReuseTCPServer(("127.0.0.1", 19854), CodexKindHandler)
    claude_thread = threading.Thread(target=claude_server.serve_forever, daemon=True)
    codex_thread = threading.Thread(target=codex_server.serve_forever, daemon=True)
    claude_thread.start()
    codex_thread.start()

    p_claude = p_codex = None
    try:
        s1, o1, r1 = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-kind-claude"),
                "kind": "anthropic",
                "base_url": "http://127.0.0.1:19853",
                "auth_ref": f"literal:{anthropic_like_openai}",
                "enabled": True,
                "priority": -1000,
                "model_aliases": [{"alias": claude_alias, "upstream_model": "claude-mock"}],
            },
        )
        if s1 != 200 or "id" not in o1:
            raise RuntimeError(f"create claude provider failed: {s1} {r1}")
        p_claude = o1["id"]

        s2, o2, r2 = _request(
            "POST",
            f"{client.base}/_vp/providers",
            {
                "name": _rid("e2e-kind-codex"),
                "kind": "openai-responses",
                "base_url": "http://127.0.0.1:19854",
                "auth_ref": f"literal:{openai_like_anthropic}",
                "enabled": True,
                "priority": -1000,
                "model_aliases": [{"alias": codex_alias, "upstream_model": "gpt-mock"}],
            },
        )
        if s2 != 200 or "id" not in o2:
            raise RuntimeError(f"create codex provider failed: {s2} {r2}")
        p_codex = o2["id"]

        c_status, _c_obj, _ = client.post_json(
            "/v1/messages",
            {"model": claude_alias, "max_tokens": 16, "messages": [{"role": "user", "content": "ping"}]},
        )
        x_status, _x_obj, _ = client.post_json(
            "/v1/responses",
            {"model": codex_alias, "input": "ping"},
        )

        header_mode_ok = (
            c_status == 200
            and x_status == 200
            and all(v == anthropic_like_openai for v in received["claude_x_api_key"] if v)
            and all(v == "" for v in received["claude_auth"])
            and all(v == "" for v in received["codex_x_api_key"])
            and all(v == f"Bearer {openai_like_anthropic}" for v in received["codex_auth"] if v)
        )
        msg = (
            f"claude_http={c_status}, codex_http={x_status}, "
            f"claude_x_api_key={received['claude_x_api_key']}, claude_auth={received['claude_auth']}, "
            f"codex_x_api_key={received['codex_x_api_key']}, codex_auth={received['codex_auth']}"
        )
        return header_mode_ok, msg
    finally:
        if p_claude:
            client.delete_provider(p_claude)
        if p_codex:
            client.delete_provider(p_codex)
        claude_server.shutdown()
        codex_server.shutdown()
        claude_server.server_close()
        codex_server.server_close()
        claude_thread.join(timeout=2)
        codex_thread.join(timeout=2)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gateway", default="http://127.0.0.1:15917")
    args = parser.parse_args()

    client = GatewayClient(args.gateway)

    tests = [
        ("protocol_routes_and_forwarding", test_protocol_routes_and_forwarding),
        ("retry_failover_500_to_200", test_retry_and_failover),
        ("error_code_failover_matrix", test_error_code_failover_matrix),
        ("payment_and_auth_semantics_expectation", test_payment_and_auth_semantics_expectation),
        ("session_affinity_or_lb_expectation", test_session_affinity_expectation),
        ("tool_path_routing_expectation", test_tool_path_routing_expectation),
        ("gemini_native_route_expectation", test_gemini_native_route_expectation),
        ("responses_protocol_semantics_expectation", test_responses_protocol_semantics_expectation),
        ("multi_auth_switching_provider_scoped", test_multi_auth_switching_provider_scoped),
        ("auth_json_ref_not_supported_yet", test_auth_json_ref_not_supported_yet),
        ("auth_header_mode_by_provider_kind_not_secret_pattern", test_auth_header_mode_by_provider_kind_not_secret_pattern),
    ]

    failed = 0
    print(f"Gateway: {args.gateway}")
    for name, fn in tests:
        try:
            ok, msg = fn(client)
        except Exception as e:  # broad on purpose for e2e runner
            ok, msg = False, f"exception: {e}"
        status = "PASS" if ok else "FAIL"
        print(f"[{status}] {name}: {msg}")
        if not ok:
            failed += 1

    if failed:
        print(f"\nResult: {failed} test(s) failed.")
        return 1
    print("\nResult: all tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
