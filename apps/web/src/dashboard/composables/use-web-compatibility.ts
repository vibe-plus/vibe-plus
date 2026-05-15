import { computed, type Ref } from "vue";
import type { Status } from "../api/client.ts";
import { WEB_COMPAT_API } from "../compat.ts";

export type WebCompatibilityState = {
  ok: boolean;
  message: string;
};

export function useWebCompatibility(
  online: Readonly<Ref<boolean>>,
  status: Readonly<Ref<Status | null>>,
) {
  return computed<WebCompatibilityState>(() => {
    const gateway = status.value?.web_compatibility;
    if (!online.value || !status.value) return { ok: true, message: "" };
    if (!gateway) {
      return {
        ok: false,
        message: `This dashboard needs Vibe CLI web API ${WEB_COMPAT_API}, but the running CLI does not report web compatibility. Please update vibe.`,
      };
    }
    if (gateway.api < WEB_COMPAT_API) {
      return {
        ok: false,
        message: `This dashboard needs Vibe CLI web API ${WEB_COMPAT_API}; running CLI ${status.value.version} provides API ${gateway.api}. Please update vibe.`,
      };
    }
    return { ok: true, message: "" };
  });
}
