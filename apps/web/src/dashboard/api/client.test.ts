import { describe, expect, test } from "vite-plus/test";
import { apiUrl, wsUrl } from "./client.ts";

describe("dashboard API URL helpers", () => {
  test("builds HTTP API URLs from the selected gateway base", () => {
    expect(apiUrl("/status", "http://127.0.0.1:15917")).toBe("http://127.0.0.1:15917/status");
  });

  test("converts gateway HTTP origins to matching WebSocket origins", () => {
    expect(wsUrl("/_vp/ws", "http://127.0.0.1:15917")).toBe("ws://127.0.0.1:15917/_vp/ws");
    expect(wsUrl("/_vp/ws", "https://web.vibe-plus.localhost")).toBe(
      "wss://web.vibe-plus.localhost/_vp/ws",
    );
  });
});
