import { readonly, ref, onMounted, onUnmounted } from "vue";
import { api, type Status, wsUrl } from "../api/client.ts";

type WsListener = (event: unknown) => void;
type StatusChangedEvent = { type: "status-changed" } & Status;
type WsSnapshotTopic =
  | "status"
  | "dashboard-stats"
  | "providers-overview"
  | "routes"
  | "codex-app-status"
  | "client-status";

const status = ref<Status | null>(null);
const online = ref(false);
const listeners = new Set<WsListener>();
const outboundQueue: string[] = [];
let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let statusPollTimer: ReturnType<typeof setTimeout> | null = null;
let refreshInFlight: Promise<void> | null = null;
let reconnectRequested = false;
let subscriberCount = 0;

function isWsUnavailable(): boolean {
  return !ws || ws.readyState === WebSocket.CLOSING || ws.readyState === WebSocket.CLOSED;
}

function clearReconnectTimer() {
  if (!reconnectTimer) return;
  clearTimeout(reconnectTimer);
  reconnectTimer = null;
}

function scheduleReconnect() {
  clearReconnectTimer();
  if (subscriberCount <= 0) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectSharedWs();
  }, 2000);
}

function clearStatusPollTimer() {
  if (!statusPollTimer) return;
  clearTimeout(statusPollTimer);
  statusPollTimer = null;
}

function scheduleStatusPoll(delayMs = online.value ? 10_000 : 2_000) {
  clearStatusPollTimer();
  if (subscriberCount <= 0) return;
  statusPollTimer = setTimeout(() => {
    statusPollTimer = null;
    void refreshStatus();
  }, delayMs);
}

async function refreshStatus() {
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = api
    .status()
    .then((next) => {
      status.value = next;
      online.value = true;
      scheduleStatusPoll();
      if (reconnectRequested && isWsUnavailable()) scheduleReconnect();
    })
    .catch(() => {
      online.value = false;
      scheduleStatusPoll(2_000);
    })
    .finally(() => {
      refreshInFlight = null;
    });
  return refreshInFlight;
}

function connectSharedWs() {
  if (ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN)) {
    return;
  }
  if (subscriberCount <= 0) return;

  reconnectRequested = true;
  ws = new WebSocket(wsUrl("/_vp/ws"));
  ws.onopen = () => {
    flushOutboundQueue();
  };
  ws.onmessage = (e) => {
    try {
      const ev = JSON.parse(e.data) as { type?: string; [k: string]: unknown };
      if (ev.type === "status-changed") {
        const { type: _type, ...next } = ev as unknown as StatusChangedEvent;
        void _type;
        status.value = next;
        online.value = true;
      }
      for (const listener of listeners) listener(ev);
    } catch {
      /* ignore malformed ws frames/listener errors */
    }
  };
  ws.onerror = () => {
    void refreshStatus();
  };
  ws.onclose = () => {
    ws = null;
    void refreshStatus();
    if (reconnectRequested) scheduleReconnect();
  };
}

function flushOutboundQueue() {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  while (outboundQueue.length > 0) {
    const next = outboundQueue.shift();
    if (next) ws.send(next);
  }
}

export function sendWsMessage(message: unknown) {
  const payload = JSON.stringify(message);
  if (ws?.readyState === WebSocket.OPEN) {
    ws.send(payload);
    return;
  }
  outboundQueue.push(payload);
  reconnectRequested = true;
  connectSharedWs();
}

export function requestWsSnapshot(
  topic: WsSnapshotTopic,
  options: { requestId?: string; hours?: number; client?: string } = {},
) {
  const requestId =
    options.requestId ??
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  sendWsMessage({
    type: "snapshot",
    request_id: requestId,
    topic,
    hours: options.hours,
    client: options.client,
  });
  return requestId;
}

function retainSharedWs() {
  subscriberCount += 1;
  reconnectRequested = true;
  scheduleStatusPoll(0);
  connectSharedWs();
}

function releaseSharedWs() {
  subscriberCount = Math.max(0, subscriberCount - 1);
  if (subscriberCount > 0) return;
  reconnectRequested = false;
  clearReconnectTimer();
  clearStatusPollTimer();
  const current = ws;
  ws = null;
  current?.close();
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    reconnectRequested = false;
    subscriberCount = 0;
    listeners.clear();
    clearReconnectTimer();
    clearStatusPollTimer();
    const current = ws;
    ws = null;
    current?.close();
  });
}

export function useProxyStatus() {
  function refresh() {
    void refreshStatus();
    requestWsSnapshot("status");
  }

  onMounted(() => {
    retainSharedWs();
    refresh();
  });
  onUnmounted(releaseSharedWs);

  return { status: readonly(status), online: readonly(online), refresh };
}

export function useWs(onEvent: (event: unknown) => void) {
  onMounted(() => {
    listeners.add(onEvent);
    retainSharedWs();
  });
  onUnmounted(() => {
    listeners.delete(onEvent);
    releaseSharedWs();
  });
}
