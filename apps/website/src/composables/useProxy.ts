import { ref, onMounted, onUnmounted } from "vue";
import { api, PORT, type Status } from "../api/client.ts";

export function useProxyStatus() {
  const status = ref<Status | null>(null);
  const online = ref(false);
  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  async function refresh() {
    try {
      status.value = await api.status();
      online.value = true;
    } catch {
      online.value = false;
      status.value = null;
    }
  }

  function connectStatusWs() {
    ws = new WebSocket(`ws://127.0.0.1:${PORT}/_vp/ws`);
    ws.onopen = () => {
      online.value = true;
    };
    ws.onmessage = (e) => {
      try {
        const ev = JSON.parse(e.data) as { type?: string; [k: string]: unknown };
        if (ev.type === "status-changed") {
          const next = { ...(ev as Record<string, unknown>) } as Partial<Status> & {
            type: "status-changed";
          };
          delete (next as Record<string, unknown>).type;
          status.value = next as Status;
          online.value = true;
        }
      } catch {
        /* ignore malformed ws frames */
      }
    };
    ws.onerror = () => {
      online.value = false;
    };
    ws.onclose = () => {
      online.value = false;
      ws = null;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      reconnectTimer = setTimeout(connectStatusWs, 2000);
    };
  }

  onMounted(() => {
    void refresh();
    connectStatusWs();
  });
  onUnmounted(() => {
    if (reconnectTimer) clearTimeout(reconnectTimer);
    ws?.close();
  });

  return { status, online, refresh };
}

export function useWs(onLog: (log: unknown) => void) {
  let ws: WebSocket | null = null;

  function connect() {
    ws = new WebSocket(`ws://127.0.0.1:${PORT}/_vp/ws`);
    ws.onmessage = (e) => {
      try {
        const ev = JSON.parse(e.data);
        if (ev.type === "log-appended") onLog(ev);
      } catch {
        /* ignore */
      }
    };
    ws.onclose = () => setTimeout(connect, 3000);
  }

  onMounted(connect);
  onUnmounted(() => ws?.close());
}
