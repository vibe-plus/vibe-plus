#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { spawnSync } from "node:child_process";
import process from "node:process";

const DEFAULT_BASE_URL = "http://127.0.0.1:15917";
const vibeRoot = process.env.VIBE_PLUS_ROOT || process.cwd();
const baseUrl = (process.env.VIBE_PLUS_BASE_URL || DEFAULT_BASE_URL).replace(/\/$/, "");

const server = new McpServer({
  name: "vibe_plus",
  version: "0.0.0",
});

const logsFilterSchema = {
  limit: z.number().int().min(1).max(100).optional(),
  offset: z.number().int().min(0).optional(),
  status: z.enum(["ok", "error"]).optional(),
  provider_id: z.string().min(1).optional(),
  since: z.number().int().optional(),
};

const summarizeSchema = {
  limit: z.number().int().min(1).max(100).optional(),
  status: z.enum(["ok", "error"]).optional(),
  provider_id: z.string().min(1).optional(),
  since: z.number().int().optional(),
  include_examples: z.boolean().optional(),
};

const detailSchema = {
  id: z.string().min(1),
  include_attempts: z.boolean().optional(),
  include_stream_trace: z.boolean().optional(),
};

const restartSchema = {
  reason: z.string().optional(),
};

type LogsFilterArgs = {
  limit?: number;
  offset?: number;
  status?: "ok" | "error";
  provider_id?: string;
  since?: number;
};

type SummarizeArgs = LogsFilterArgs & { include_examples?: boolean };
type DetailArgs = { id: string; include_attempts?: boolean; include_stream_trace?: boolean };

function ageSeconds(startedAt: number): number {
  return Math.max(0, Math.floor(Date.now() / 1000) - startedAt);
}

function buildLogsUrl(args: LogsFilterArgs): URL {
  const limit = Math.min(100, Math.max(1, args.limit ?? 20));
  const offset = Math.max(0, args.offset ?? 0);
  const url = new URL(`${baseUrl}/_vp/logs`);
  url.searchParams.set("limit", String(limit));
  if (offset > 0) url.searchParams.set("offset", String(offset));
  if (args.status) url.searchParams.set("status", args.status);
  if (args.provider_id) url.searchParams.set("provider_id", args.provider_id);
  if (args.since != null) url.searchParams.set("since", String(args.since));
  return url;
}

async function fetchJson(url: URL | string): Promise<any> {
  const res = await fetch(url, { headers: { accept: "application/json" } });
  if (!res.ok) throw new Error(`gateway error: ${res.status} ${res.statusText}`);
  return await res.json();
}

server.registerTool(
  "list_logs",
  {
    description:
      "List recent Vibe Plus gateway logs in a lightweight format suitable for agent triage.",
    inputSchema: logsFilterSchema,
  },
  async (args: LogsFilterArgs) => {
    const page = await fetchJson(buildLogsUrl(args));
    const items = Array.isArray(page.items) ? page.items : [];
    const normalized = items.map((item: any) => ({
      id: item.id ?? null,
      started_at: item.started_at ?? null,
      age_sec: typeof item.started_at === "number" ? ageSeconds(item.started_at) : null,
      provider_id: item.provider_id ?? null,
      requested_model: item.requested_model ?? null,
      upstream_model: item.upstream_model ?? null,
      status_code: item.status_code ?? null,
      ok:
        typeof item.status_code === "number"
          ? item.status_code >= 200 && item.status_code < 300
          : null,
      latency_ms: item.latency_ms ?? null,
      input_tokens: item.input_tokens ?? null,
      output_tokens: item.output_tokens ?? null,
      error: item.error ?? null,
      wire: item.wire ?? null,
      route_prefix: item.route_prefix ?? null,
    }));
    const result = {
      filters: args,
      total: page.total ?? null,
      limit: page.limit ?? null,
      offset: page.offset ?? null,
      items: normalized,
    };
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      structuredContent: result,
    };
  },
);

server.registerTool(
  "summarize_logs",
  {
    description:
      "Summarize Vibe Plus gateway logs by count, provider, model, status, and representative failures.",
    inputSchema: summarizeSchema,
  },
  async (args: SummarizeArgs) => {
    const filter: LogsFilterArgs = {
      limit: args.limit,
      offset: 0,
      status: args.status,
      provider_id: args.provider_id,
      since: args.since,
    };
    const page = await fetchJson(buildLogsUrl(filter));
    const items = Array.isArray(page.items) ? page.items : [];
    const byProvider: Record<string, number> = {};
    const byModel: Record<string, number> = {};
    const byStatus: Record<string, number> = { ok: 0, error: 0 };
    const examples: any[] = [];

    for (const item of items) {
      const provider = item.provider_id || "unknown";
      const model = item.upstream_model || item.requested_model || "unknown";
      byProvider[provider] = (byProvider[provider] || 0) + 1;
      byModel[model] = (byModel[model] || 0) + 1;
      const ok =
        typeof item.status_code === "number" && item.status_code >= 200 && item.status_code < 300;
      byStatus[ok ? "ok" : "error"] += 1;
      if (args.include_examples && examples.length < 5) {
        examples.push({
          id: item.id ?? null,
          provider_id: item.provider_id ?? null,
          requested_model: item.requested_model ?? null,
          upstream_model: item.upstream_model ?? null,
          status_code: item.status_code ?? null,
          error: item.error ?? null,
          latency_ms: item.latency_ms ?? null,
          started_at: item.started_at ?? null,
        });
      }
    }

    const result = {
      filters: args,
      total: page.total ?? items.length,
      window_count: items.length,
      by_provider: byProvider,
      by_model: byModel,
      by_status: byStatus,
      examples,
    };
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      structuredContent: result,
    };
  },
);

server.registerTool(
  "get_log_detail",
  {
    description:
      "Fetch one full Vibe Plus log entry, optionally including upstream attempts and stream trace diagnostics.",
    inputSchema: detailSchema,
  },
  async (args: DetailArgs) => {
    const detail = await fetchJson(`${baseUrl}/_vp/logs/${encodeURIComponent(args.id)}`);
    const attempts = args.include_attempts
      ? await fetchJson(`${baseUrl}/_vp/logs/${encodeURIComponent(args.id)}/attempts`)
      : null;
    const stream_trace = args.include_stream_trace
      ? await fetchJson(`${baseUrl}/_vp/logs/${encodeURIComponent(args.id)}/stream-trace`)
      : null;
    const result = { detail, attempts, stream_trace };
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      structuredContent: result,
    };
  },
);

server.registerTool(
  "restart_gateway",
  {
    description:
      "Restart the Vibe Plus gateway by running `bun gateway:restart` in the installed Vibe Plus directory.",
    inputSchema: restartSchema,
  },
  async (args: { reason?: string }) => {
    const output = spawnSync("bun", ["gateway:restart"], {
      cwd: vibeRoot,
      encoding: "utf8",
    });
    const result = {
      ok: output.status === 0,
      reason: args.reason ?? null,
      cwd: vibeRoot,
      status: output.status,
      stdout: output.stdout ?? "",
      stderr: output.stderr ?? "",
    };
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      structuredContent: result,
    };
  },
);

const transport = new StdioServerTransport();
await server.connect(transport);
