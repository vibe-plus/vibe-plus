import { readonly, ref, onMounted, onUnmounted } from "vue";
import { api, type Status } from "../api/client.ts";

const status = ref<Status | null>(null);
const online = ref(false);
let statusPollTimer: ReturnType<typeof setTimeout> | null = null;
let refreshInFlight: Promise<void> | null = null;
let subscriberCount = 0;

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

function retainStatusPolling() {
  subscriberCount += 1;
  scheduleStatusPoll(0);
}

function releaseStatusPolling() {
  subscriberCount = Math.max(0, subscriberCount - 1);
  if (subscriberCount > 0) return;
  clearStatusPollTimer();
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    subscriberCount = 0;
    clearStatusPollTimer();
  });
}

export function useProxyStatus() {
  function refresh() {
    void refreshStatus();
  }

  onMounted(() => {
    retainStatusPolling();
    refresh();
  });
  onUnmounted(releaseStatusPolling);

  return { status: readonly(status), online: readonly(online), refresh };
}
