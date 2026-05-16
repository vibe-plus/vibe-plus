import { describe, expect, test } from "vite-plus/test";
import {
  codex_request_overview_fields,
  stream_trace_line_diff,
  trace_diff_rows_for_clipboard,
} from "./codex-link-trace.ts";

describe("codex link trace helpers", () => {
  test("builds compact line diff and clipboard output", () => {
    const result = stream_trace_line_diff("a\nb\n", "a\nc\n", { max_rows: 10 });

    expect(result.diff_aborted).toBe(false);
    expect(result.clipped_input).toBe(false);
    expect(result.truncated).toBe(false);
    expect(result.rows).toEqual([
      { mark: " ", text: "a" },
      { mark: "-", text: "b" },
      { mark: "+", text: "c" },
    ]);
    expect(trace_diff_rows_for_clipboard(result.rows)).toBe("  a\n- b\n+ c");
  });

  test("normalizes CRLF and reports row truncation", () => {
    const result = stream_trace_line_diff("a\r\nb\r\nc\r\n", "a\r\nB\r\nc\r\n", { max_rows: 2 });
    expect(result.rows).toHaveLength(2);
    expect(result.truncated).toBe(true);
  });

  test("returns overview fields with stable fallback labels", () => {
    const fields = codex_request_overview_fields({
      app: null,
      client_transport: "http-sse",
      wire: "openai-responses",
      route_prefix: "/codex/v1",
      requested_model: "gpt-5",
      upstream_model: null,
      provider_id: "provider-1",
      status_code: 200,
      upstream_http_status: null,
      latency_ms: 123,
      upstream_first_byte_ms: undefined,
      first_token_ms: 45,
      client_first_write_ms: null,
      input_tokens: 10,
      output_tokens: 20,
      cache_read_tokens: 3,
      cache_creation_tokens: 4,
      estimated_cost_usd: "0.001",
      upstream_chunk_count: 2,
      upstream_bytes: 1000,
      client_chunk_count: 3,
      client_bytes: 2000,
      stream_kind: "sse",
      stream_end_reason: null,
      stream_terminal_seen: true,
      upstream_terminal_type: "response.completed",
      status_injected: false,
      terminal_injected: true,
      bridge_mode: null,
      dedupe_key: "dedupe-1",
      cb_key: null,
      credential_id: "cred-1",
      stream_error_detail: "tail parse failed",
    } as any);

    expect(fields).toContainEqual({ label: "App", value: "—" });
    expect(fields).toContainEqual({ label: "Client transport", value: "http-sse" });
    expect(fields).toContainEqual({ label: "in / out tokens", value: "10 / 20" });
    expect(fields).toContainEqual({ label: "Upstream chunks / bytes", value: "2 / 1,000" });
    expect(fields).toContainEqual({ label: "Stream error detail", value: "tail parse failed" });
  });
});
