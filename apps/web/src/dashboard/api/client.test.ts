import { describe, expect, test } from "vite-plus/test";
import { apiUrl } from "./client.ts";

describe("dashboard API URL helpers", () => {
  test("builds HTTP API URLs from the selected gateway base", () => {
    expect(apiUrl("/status", "http://127.0.0.1:15917")).toBe("http://127.0.0.1:15917/status");
  });
});
