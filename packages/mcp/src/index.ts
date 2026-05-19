#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { spawnSync } from "node:child_process";
import process from "node:process";
import { z } from "zod";

const vibeRoot = process.env.VIBE_PLUS_ROOT || process.cwd();

const server = new McpServer({
  name: "vibe_plus",
  version: "0.0.0",
});

function resolveGatewayBaseUrl(): string {
  const raw = process.env.VIBE_PLUS_GATEWAY_URL || process.env.VIBE_PLUS_URL || "";
  if (raw.trim()) return raw.trim().replace(/\/$/, "");
  const port = process.env.VIBE_PLUS_PORT || "15917";
  return `http://127.0.0.1:${port}`;
}

const gatewayBaseUrl = resolveGatewayBaseUrl();

type GatewayQueryValue = string | number | boolean | bigint;
type GatewayQuery = Record<string, GatewayQueryValue | null | undefined>;

async function gatewayGet(path: string, query?: GatewayQuery) {
  const url = new URL(path, gatewayBaseUrl);
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value === undefined || value === null || value === "") continue;
    url.searchParams.set(key, String(value));
  }
  const res = await fetch(url);
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`${res.status} ${text}`);
  }
  return text.trim() ? JSON.parse(text) : null;
}

function jsonToolResult(result: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(result, null, 2) }],
    structuredContent: result as Record<string, unknown>,
  };
}

const pageSchema = {
  limit: z.number().int().min(1).max(1000).optional(),
  offset: z.number().int().min(0).optional(),
};

server.registerTool(
  "list_usage_rollups",
  {
    description:
      "List long-retention daily usage/cost/reliability rollups. Use this for yearly tokens, USD spend, requests by upstream/provider, success rate, and latency even after raw logs are pruned.",
    inputSchema: {
      ...pageSchema,
      since_day: z.string().optional(),
      until_day: z.string().optional(),
      scope: z.enum(["request", "upstream_attempt"]).optional(),
      provider_id: z.string().optional(),
      credential_id: z.string().optional(),
      upstream_id: z.string().optional(),
    },
  },
  async (args: {
    limit?: number;
    offset?: number;
    since_day?: string;
    until_day?: string;
    scope?: "request" | "upstream_attempt";
    provider_id?: string;
    credential_id?: string;
    upstream_id?: string;
  }) => {
    return jsonToolResult(await gatewayGet("/_vp/stats/usage-rollups", args));
  },
);

server.registerTool(
  "list_app_logs",
  {
    description:
      "List Vibe+ application/runtime logs (operator events, circuit events, provider/credential changes). Not network traffic.",
    inputSchema: {
      limit: z.number().int().min(1).max(500).optional(),
      since: z.number().int().optional(),
    },
  },
  async (args: { limit?: number; since?: number }) => {
    return jsonToolResult(await gatewayGet("/_vp/logs/app", args));
  },
);

server.registerTool(
  "list_request_records",
  {
    description:
      "List gateway request records. Each item is one inbound user/client request, not each upstream network attempt.",
    inputSchema: {
      ...pageSchema,
      since: z.number().int().optional(),
      provider_id: z.string().optional(),
      status_ok: z.boolean().optional(),
    },
  },
  async (args: {
    limit?: number;
    offset?: number;
    since?: number;
    provider_id?: string;
    status_ok?: boolean;
  }) => {
    return jsonToolResult(await gatewayGet("/_vp/records/requests", args));
  },
);

server.registerTool(
  "get_request_record",
  {
    description:
      "Get one full gateway request record by request id, including stored request/response bodies when logging is enabled.",
    inputSchema: { request_id: z.string().min(1) },
  },
  async (args: { request_id: string }) => {
    return jsonToolResult(
      await gatewayGet(`/_vp/records/requests/${encodeURIComponent(args.request_id)}`),
    );
  },
);

server.registerTool(
  "list_request_network_records",
  {
    description:
      "List upstream network attempts for one request id. Use wave_index/wave_size to reconstruct request -> waves -> upstreams.",
    inputSchema: { request_id: z.string().min(1) },
  },
  async (args: { request_id: string }) => {
    return jsonToolResult(
      await gatewayGet(`/_vp/records/requests/${encodeURIComponent(args.request_id)}/network`),
    );
  },
);

server.registerTool(
  "list_network_attempt_records",
  {
    description:
      "List raw upstream network attempt records across requests, newest first. These are per-upstream attempts, not user requests and not app logs.",
    inputSchema: pageSchema,
  },
  async (args: { limit?: number; offset?: number }) => {
    return jsonToolResult(await gatewayGet("/_vp/records/network-attempts", args));
  },
);

const restartSchema = {
  reason: z.string().optional(),
};

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
