#!/usr/bin/env node
/**
 * Smoke-check hosted UI URLs (GitHub Pages project site).
 * Usage: node scripts/check-hosted-ui-urls.mjs
 */
import { readFileSync } from "node:fs";

const base = "https://vibe-plus.github.io/vibe-plus";
const dashboard = `${base}/ui/overview`;

const checks = [
  { url: `${base}/`, expect: [200], label: "site root" },
  { url: `${base}/version.json`, expect: [200], label: "version.json" },
  { url: `${base}/index.html`, expect: [200], label: "index.html" },
  { url: dashboard, expect: [404], label: "dashboard deep link (404.html SPA)" },
  { url: "https://vibe-plus.github.io/ui", expect: [404], label: "legacy wrong path" },
];

let failed = 0;

for (const { url, expect, label } of checks) {
  const res = await fetch(url, { redirect: "follow" });
  const ok = expect.includes(res.status);
  console.log(`${ok ? "ok" : "FAIL"} [${res.status}] ${label}: ${url}`);
  if (!ok) failed += 1;
}

const localVersion = JSON.parse(readFileSync("apps/web/package.json", "utf8")).version;
const remoteVersion = await fetch(`${base}/version.json`)
  .then((r) => r.json())
  .then((j) => j.version);
if (localVersion !== remoteVersion) {
  console.log(
    `FAIL version drift: apps/web is ${localVersion}, Pages serves ${remoteVersion} (redeploy Pages after bump)`,
  );
  failed += 1;
} else {
  console.log(`ok version.json matches apps/web (${localVersion})`);
}

process.exit(failed > 0 ? 1 : 0);
