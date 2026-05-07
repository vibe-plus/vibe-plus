import { ref, onMounted, onUnmounted } from "vue";
import { api, PORT, type Status } from "../api/client.ts";

export function useProxyStatus() {
  const status = ref<Status | null>(null);
  const online = ref(false);
  let timer: ReturnType<typeof setInterval>;

  async function refresh() {
    try {
      status.value = await api.status();
      online.value = true;
    } catch {
      online.value = false;
      status.value = null;
    }
  }

  onMounted(() => {
    void refresh();
    timer = setInterval(refresh, 5000);
  });
  onUnmounted(() => clearInterval(timer));

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
