import { diffLines } from "diff";
import type { RequestLog } from "../api/client.ts";

export type trace_diff_row = {
  mark: " " | "+" | "-";
  text: string;
};

export type trace_diff_result = {
  rows: trace_diff_row[];
  /** stopped listing rows at limit */
  truncated: boolean;
  /** diff hit maxEditLength and gave up */
  diff_aborted: boolean;
  /** upstream/client trace was clipped before diff */
  clipped_input: boolean;
};

const max_trace_chars = 480_000;
const default_max_diff_rows = 6_000;
const default_max_edit_length = 120_000;

/** 「上游→网关」与「网关→Codex」逐行 unified diff，便于看 summary 注入、终止帧等。 */
export function stream_trace_line_diff(
  upstream_text: string | null | undefined,
  client_text: string | null | undefined,
  opts?: { max_rows?: number; max_edit_length?: number },
): trace_diff_result {
  const max_rows = opts?.max_rows ?? default_max_diff_rows;
  const max_edit_length = opts?.max_edit_length ?? default_max_edit_length;
  let up = (upstream_text ?? "").split("\r\n").join("\n");
  let down = (client_text ?? "").split("\r\n").join("\n");
  let clipped_input = false;
  if (up.length > max_trace_chars) {
    up = up.slice(0, max_trace_chars);
    clipped_input = true;
  }
  if (down.length > max_trace_chars) {
    down = down.slice(0, max_trace_chars);
    clipped_input = true;
  }

  const parts = diffLines(up, down, { maxEditLength: max_edit_length });
  if (parts === undefined) {
    return {
      rows: [],
      truncated: false,
      diff_aborted: true,
      clipped_input,
    };
  }

  const rows: trace_diff_row[] = [];
  let truncated = false;
  for (const p of parts) {
    const mark: trace_diff_row["mark"] = p.added ? "+" : p.removed ? "-" : " ";
    const raw = p.value.replace(/\n$/, "");
    const lines = raw.length === 0 ? [""] : raw.split("\n");
    for (const line of lines) {
      if (rows.length >= max_rows) {
        truncated = true;
        break;
      }
      rows.push({ mark, text: line });
    }
    if (truncated) break;
  }
  return { rows, truncated, diff_aborted: false, clipped_input };
}

export function trace_diff_rows_for_clipboard(rows: trace_diff_row[]): string {
  return rows.map((r) => `${r.mark} ${r.text}`).join("\n");
}

export type overview_field = { label: string; value: string };

function pick(n: number | null | undefined): string {
  return n == null ? "—" : String(n);
}

/** 类似 Chrome「时间 + 启动器」摘要：单屏看清链路与流式健康。 */
export function codex_request_overview_fields(log: RequestLog): overview_field[] {
  const rows: overview_field[] = [
    { label: "应用", value: log.app ?? "—" },
    { label: "客户端传输", value: log.client_transport ?? "—" },
    { label: "wire", value: log.wire ?? "—" },
    { label: "route_prefix", value: log.route_prefix ?? "—" },
    { label: "请求模型", value: log.requested_model ?? "—" },
    { label: "上游模型", value: log.upstream_model ?? "—" },
    { label: "提供商", value: log.provider_id ?? "—" },
    { label: "HTTP 状态", value: pick(log.status_code) },
    { label: "上游 HTTP", value: pick(log.upstream_http_status) },
    { label: "总耗时 ms", value: pick(log.latency_ms) },
    { label: "上游首包 ms", value: pick(log.upstream_first_byte_ms) },
    { label: "首 token ms", value: pick(log.first_token_ms) },
    { label: "发往客户端首写 ms", value: pick(log.client_first_write_ms) },
    { label: "in / out tokens", value: `${log.input_tokens} / ${log.output_tokens}` },
    { label: "cache 读 / 写", value: `${log.cache_read_tokens} / ${log.cache_creation_tokens}` },
    { label: "估算 $", value: log.estimated_cost_usd ?? "—" },
    {
      label: "上游块数 / 字节",
      value: `${log.upstream_chunk_count ?? 0} / ${(log.upstream_bytes ?? 0).toLocaleString()}`,
    },
    {
      label: "客户端块数 / 字节",
      value: `${log.client_chunk_count ?? 0} / ${(log.client_bytes ?? 0).toLocaleString()}`,
    },
    { label: "stream_kind", value: log.stream_kind ?? "—" },
    { label: "stream 结束原因", value: log.stream_end_reason ?? "—" },
    {
      label: "已见上游终端",
      value: log.stream_terminal_seen == null ? "—" : String(log.stream_terminal_seen),
    },
    { label: "上游终端 type", value: log.upstream_terminal_type ?? "—" },
    { label: "网关注入 status 帧", value: String(log.status_injected ?? false) },
    { label: "网关注入终端帧", value: String(log.terminal_injected ?? false) },
    { label: "bridge_mode", value: log.bridge_mode ?? "—" },
    { label: "dedupe", value: log.dedupe_key ?? "—" },
    { label: "cb_key", value: log.cb_key ?? "—" },
    { label: "credential_id", value: log.credential_id ?? "—" },
  ];
  if (log.stream_error_detail) {
    rows.push({ label: "stream 错误详情", value: log.stream_error_detail });
  }
  return rows;
}

export function frame_type_counts(trace: string | null | undefined): Map<string, number> {
  const m = new Map<string, number>();
  const lines = (trace ?? "").split("\n").filter((ln) => ln.trim().length > 0);
  for (const line of lines) {
    let t = "(non-json)";
    try {
      const o = JSON.parse(line) as { type?: string };
      if (typeof o.type === "string") t = o.type;
    } catch {
      /* keep (non-json) */
    }
    m.set(t, (m.get(t) ?? 0) + 1);
  }
  return m;
}
