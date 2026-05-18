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
