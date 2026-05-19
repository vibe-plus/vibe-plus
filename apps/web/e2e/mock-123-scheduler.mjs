import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import http from "node:http";

const root = decodeURIComponent(new URL("../../..", import.meta.url).pathname).replace(/\/$/, "");
const keep = process.env.KEEP_E2E_ENV === "1";
const tmpRoot = await mkdtemp(join(tmpdir(), "vibe-plus-e2e-123-"));
const home = join(tmpRoot, "home");
const codexHome = join(tmpRoot, "codex");
const claudeHome = join(tmpRoot, "claude");
const xdgHome = join(tmpRoot, "xdg");
await Promise.all([home, codexHome, claudeHome, xdgHome].map((p) => mkdir(p, { recursive: true })));

const events = [];
let seq = 0;
const mock = http.createServer((req, res) => {
  const id = req.headers["x-mock-id"] ?? `unknown-${seq++}`;
  const url = new URL(req.url ?? "/", "http://mock.local");
  if (req.method !== "POST" || url.pathname !== "/v1/chat/completions") {
    res.writeHead(404, { "content-type": "application/json" });
    res.end(JSON.stringify({ error: "mock route not found" }));
    return;
  }
  const n = seq++;
  const delay = id === "healthy-2" ? 200 : id === "healthy-3" ? 20 : id === "healthy-4" ? 10 : 50;
  events.push({ id, n, at: Date.now(), delay });
  setTimeout(() => {
    if (id === "healthy-4") {
      res.writeHead(200, { "content-type": "application/json", "x-mock-winner": id });
      res.end(JSON.stringify({ id: "chatcmpl-mock", object: "chat.completion", choices: [] }));
    } else {
      res.writeHead(500, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: `planned failure from ${id}` }));
    }
  }, delay);
});

const mockPort = await listen(mock);
const gatewayPort = await getFreePort();
const env = {
  ...process.env,
  VIBE_HOME: home,
  CODEX_HOME: codexHome,
  CLAUDE_CONFIG_DIR: claudeHome,
  XDG_CONFIG_HOME: xdgHome,
  RUST_LOG: process.env.RUST_LOG ?? "warn",
};

let gateway;
try {
  gateway = spawn(
    join(root, "target/debug/vibe"),
    ["up", "--foreground", "--port", String(gatewayPort)],
    {
      cwd: root,
      env,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  gateway.once("exit", (code, signal) => {
    if (code !== 0 && !gateway.killed) {
      console.error(`isolated gateway exited early: code=${code} signal=${signal}`);
    }
  });
  const gatewayLogs = [];
  gateway.stdout.on("data", (b) => gatewayLogs.push(b.toString()));
  gateway.stderr.on("data", (b) => gatewayLogs.push(b.toString()));
  await waitForHttp(`http://127.0.0.1:${gatewayPort}/status`, 15_000, () => gatewayLogs.join(""));

  const base = `http://127.0.0.1:${gatewayPort}`;
  const providerIds = [];
  for (const id of [
    "healthy-0",
    "sick-0",
    "healthy-1",
    "sick-1",
    "healthy-2",
    "healthy-3",
    "healthy-4",
    "extra-0",
    "extra-1",
    "extra-2",
    "extra-3",
    "extra-4",
  ]) {
    const provider = await postJson(`${base}/_vp/providers`, {
      name: id,
      group_name: null,
      avatar_url: null,
      kind: "openai-chat",
      base_url: `http://127.0.0.1:${mockPort}`,
      protocols: [],
      host: null,
      auth_ref: "passthrough",
      enabled: true,
      priority: 10,
      supports_websocket: null,
      passthrough_mode: true,
      model_aliases: [],
    });
    providerIds.push([id, provider.id]);
  }

  for (const id of ["sick-0", "sick-1"]) {
    for (let i = 0; i < 3; i++) {
      await postJsonAllowError(
        `${base}/v1/chat/completions`,
        { model: "gpt-mock", messages: [{ role: "user", content: "open cb" }] },
        { "x-api-key": "test", "x-mock-id": id },
      );
    }
  }

  events.length = 0;
  const finalResult = await postJsonAllowError(
    `${base}/v1/chat/completions`,
    { model: "gpt-mock", messages: [{ role: "user", content: "run 123" }] },
    { "x-api-key": "test", "x-mock-id": "healthy-4" },
  );
  if (finalResult.ok) {
    const resp = JSON.parse(finalResult.text);
    if (resp.id !== "chatcmpl-mock")
      throw new Error(`unexpected gateway response ${JSON.stringify(resp)}`);
  } else if (finalResult.status !== 503) {
    throw new Error(`unexpected final status ${finalResult.status}: ${finalResult.text}`);
  }

  const records = await getJson(`${base}/_vp/observability/requests?limit=5`);
  const request = records.items?.[0] ?? records.rows?.[0] ?? records[0];
  if (!request?.id) throw new Error(`cannot find request record: ${JSON.stringify(records)}`);
  const attempts = await getJson(`${base}/_vp/observability/requests/${request.id}/network`);
  const byWave = Map.groupBy(attempts, (a) => a.wave_index);
  const summary = [...byWave.entries()]
    .map(([wave, rows]) => ({
      wave,
      size: rows[0]?.wave_size,
      attempts: rows
        .map((a) => ({
          attempt_index: a.attempt_index,
          provider: providerIds.find(([, pid]) => pid === a.provider_id)?.[0] ?? a.provider_id,
          outcome: a.outcome,
          status: a.status_code,
          wave_size: a.wave_size,
          route_prefix: a.route_prefix,
          wire: a.wire,
          requested_model: a.requested_model,
          upstream_id: a.upstream_id,
        }))
        .sort((a, b) => a.attempt_index - b.attempt_index),
    }))
    .sort((a, b) => a.wave - b.wave);

  assertEqual(
    summary.map((w) => w.size),
    [1, 2, 3],
    "wave sizes",
  );
  assertEqual(
    summary.map((w) => w.attempts.length),
    [1, 2, 3],
    "attempts per wave",
  );
  if (summary.length !== 3)
    throw new Error(
      `expected exactly 3 waves, got ${summary.length}: ${JSON.stringify(summary, null, 2)}`,
    );
  const tried = summary.flatMap((w) => w.attempts.map((a) => a.provider));
  const allProviders = providerIds.map(([name]) => name);
  const untried = allProviders.filter((name) => !tried.includes(name));
  if (untried.length < allProviders.length - 6) {
    throw new Error(
      `expected at least ${allProviders.length - 6} untried providers after 123 cap; got ${JSON.stringify(untried)} in ${JSON.stringify(summary, null, 2)}`,
    );
  }
  if (finalResult.ok) {
    const winner = summary.flatMap((w) => w.attempts).find((a) => a.outcome === "success");
    if (winner?.provider !== "healthy-4" || summary.at(-1).wave !== 2) {
      throw new Error(`expected healthy-4 to win in wave 2: ${JSON.stringify(summary, null, 2)}`);
    }
  } else {
    if (summary.flatMap((w) => w.attempts).some((a) => a.outcome === "success")) {
      throw new Error(
        `503 scenario should not have success attempts: ${JSON.stringify(summary, null, 2)}`,
      );
    }
  }
  for (const attempt of summary.flatMap((w) => w.attempts)) {
    if (attempt.wire !== "openai-chat") throw new Error(`bad wire: ${JSON.stringify(attempt)}`);
    if (attempt.route_prefix !== "plain-v1")
      throw new Error(`bad route_prefix: ${JSON.stringify(attempt)}`);
    if (attempt.requested_model !== "gpt-mock")
      throw new Error(`bad requested_model: ${JSON.stringify(attempt)}`);
    if (!attempt.upstream_id) throw new Error(`missing upstream_id: ${JSON.stringify(attempt)}`);
  }

  console.log(
    JSON.stringify(
      { ok: true, tmpRoot, gatewayPort, mockPort, summary, mockEvents: events },
      null,
      2,
    ),
  );
} finally {
  if (gateway && !gateway.killed) {
    gateway.kill("SIGTERM");
    await onceExit(gateway, 5000).catch(() => gateway.kill("SIGKILL"));
  }
  mock.close();
  if (!keep) await rm(tmpRoot, { recursive: true, force: true });
}

async function listen(server) {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return server.address().port;
}
async function getFreePort() {
  const s = http.createServer();
  const port = await listen(s);
  await new Promise((resolve) => s.close(resolve));
  return port;
}
async function waitForHttp(url, timeoutMs, logs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (gateway?.exitCode !== null)
      throw new Error(`gateway exited while waiting for ${url}: code=${gateway.exitCode} signal=${gateway.signalCode}
${logs()}`);
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error(`timeout waiting for ${url}\n${logs()}`);
}
async function postJson(url, body, headers = {}) {
  const r = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`${url} -> ${r.status} ${text}`);
  return text ? JSON.parse(text) : null;
}
async function postJsonAllowError(url, body, headers = {}) {
  const r = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
  const text = await r.text();
  return { ok: r.ok, status: r.status, text };
}
async function getJson(url) {
  const r = await fetch(url);
  const text = await r.text();
  if (!r.ok) throw new Error(`${url} -> ${r.status} ${text}`);
  return text ? JSON.parse(text) : null;
}
function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}
function onceExit(child, timeoutMs) {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("exit timeout")), timeoutMs);
    child.once("exit", (code, signal) => {
      clearTimeout(t);
      resolve({ code, signal });
    });
  });
}
function assertEqual(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}
