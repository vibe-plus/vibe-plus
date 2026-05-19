import { onMounted, onUnmounted, ref, shallowRef, type Ref } from "vue";
import { api, apiUrl, type RealtimeSnapshot } from "../api/client.ts";

export type RealtimeTransport = "connecting" | "stream" | "polling" | "offline";

export interface UseRealtimeStreamResult {
  snapshot: Ref<RealtimeSnapshot | null>;
  transport: Ref<RealtimeTransport>;
  /** Force a one-off refresh — useful right after an action that should bump KPIs. */
  refresh: () => Promise<void>;
}

const POLL_FALLBACK_INTERVAL_MS = 1_000;
const RECONNECT_BACKOFF_MS = [500, 1_000, 2_000, 5_000];

/**
 * Subscribe to `/_vp/stream/realtime` via SSE, with HTTP-poll fallback.
 *
 * Connection lifecycle:
 *  - mount → open EventSource, set transport="connecting"
 *  - first frame → transport="stream"
 *  - error → close stream, retry with backoff; after the backoff table runs
 *    out we drop to HTTP polling and stay there until unmount.
 *
 * The fallback path is what keeps things working on older gateway binaries
 * (the SSE route only exists from this build forward).
 */
export function useRealtimeStream(): UseRealtimeStreamResult {
  const snapshot = shallowRef<RealtimeSnapshot | null>(null);
  const transport = ref<RealtimeTransport>("connecting");

  let source: EventSource | null = null;
  let pollTimer: number | null = null;
  let reconnectTimer: number | null = null;
  let reconnectAttempt = 0;
  let stopped = false;

  function clearPoll() {
    if (pollTimer !== null) {
      window.clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  function clearReconnect() {
    if (reconnectTimer !== null) {
      window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function closeStream() {
    if (source) {
      source.close();
      source = null;
    }
  }

  async function pollOnce() {
    try {
      snapshot.value = await api.realtime();
      transport.value = "polling";
    } catch {
      transport.value = "offline";
    }
  }

  function startPolling() {
    clearPoll();
    void pollOnce();
    pollTimer = window.setInterval(() => {
      if (document.visibilityState === "hidden") return;
      void pollOnce();
    }, POLL_FALLBACK_INTERVAL_MS);
  }

  function scheduleReconnect() {
    clearReconnect();
    if (stopped) return;
    if (reconnectAttempt >= RECONNECT_BACKOFF_MS.length) {
      startPolling();
      return;
    }
    const delay = RECONNECT_BACKOFF_MS[reconnectAttempt]!;
    reconnectAttempt += 1;
    reconnectTimer = window.setTimeout(connectStream, delay);
  }

  function connectStream() {
    if (stopped) return;
    clearPoll();
    closeStream();
    transport.value = "connecting";

    try {
      source = new EventSource(apiUrl("/_vp/stream/realtime"));
    } catch {
      scheduleReconnect();
      return;
    }

    const handle = (raw: string) => {
      try {
        snapshot.value = JSON.parse(raw) as RealtimeSnapshot;
        transport.value = "stream";
        reconnectAttempt = 0;
      } catch {
        // ignore single bad frame — next tick will overwrite
      }
    };

    source.addEventListener("snapshot", (ev) => handle((ev as MessageEvent).data));
    source.onmessage = (ev) => handle(ev.data);
    source.onerror = () => {
      // EventSource auto-reconnects on its own for transient errors; but if
      // the connection never opened we want to switch to polling instead of
      // letting the browser hammer a missing route silently.
      closeStream();
      scheduleReconnect();
    };
  }

  async function refresh() {
    try {
      snapshot.value = await api.realtime();
    } catch {
      // leave previous snapshot in place
    }
  }

  onMounted(() => {
    stopped = false;
    connectStream();
  });

  onUnmounted(() => {
    stopped = true;
    clearReconnect();
    clearPoll();
    closeStream();
  });

  return { snapshot, transport, refresh };
}
