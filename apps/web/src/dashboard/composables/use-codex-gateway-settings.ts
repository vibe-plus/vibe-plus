import { computed, ref, watch } from "vue";
import { api, type CodexGatewaySettings, type CodexSummaryConfig } from "../api/client.ts";

function defaultSummary(): CodexSummaryConfig {
  return {
    enabled: true,
    show_speed: true,
    show_input: true,
    show_output: true,
    show_cache: true,
    show_latency: false,
    show_first_token: false,
    speed_decimal_places: 1,
    separator: " · ",
    label_overrides: {},
    clients: {
      app: { enabled: true, style: "formula_compact", prefix: null, suffix: null },
      cli: { enabled: true, style: "plain_compact", prefix: null, suffix: null },
      unknown: { enabled: true, style: "plain_compact", prefix: null, suffix: null },
    },
  };
}

function cloneSettings(value: CodexGatewaySettings): CodexGatewaySettings {
  return structuredClone(value);
}

export function useCodexGatewaySettings() {
  const loading = ref(false);
  const saving = ref(false);
  const error = ref<string | null>(null);
  const saved = ref<CodexGatewaySettings | null>(null);
  const draft = ref<CodexGatewaySettings>({
    summary: defaultSummary(),
    route_status_enabled: true,
  });

  const dirty = computed(() => {
    if (!saved.value) return false;
    return JSON.stringify(saved.value) !== JSON.stringify(draft.value);
  });

  async function refresh() {
    loading.value = true;
    error.value = null;
    try {
      const data = await api.config.getCodex();
      saved.value = cloneSettings(data);
      draft.value = cloneSettings(data);
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  async function save() {
    saving.value = true;
    error.value = null;
    try {
      const data = await api.config.saveCodex(draft.value);
      saved.value = cloneSettings(data);
      draft.value = cloneSettings(data);
    } catch (e) {
      error.value = String(e);
    } finally {
      saving.value = false;
    }
  }

  function reset() {
    if (!saved.value) return;
    draft.value = cloneSettings(saved.value);
  }

  watch(
    () => draft.value,
    () => {
      if (error.value && dirty.value) error.value = null;
    },
    { deep: true },
  );

  const endRecap = computed({
    get: () => draft.value.summary,
    set: (summary: CodexSummaryConfig) => {
      draft.value = { ...draft.value, summary };
    },
  });

  const routeStatusEnabled = computed({
    get: () => draft.value.route_status_enabled,
    set: (route_status_enabled: boolean) => {
      draft.value = { ...draft.value, route_status_enabled };
    },
  });

  return {
    loading,
    saving,
    error,
    dirty,
    endRecap,
    routeStatusEnabled,
    refresh,
    save,
    reset,
  };
}
