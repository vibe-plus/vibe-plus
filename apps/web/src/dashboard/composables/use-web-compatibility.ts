import { computed, type Ref } from "vue";
import type { Status } from "../api/client.ts";
import { BRAND_NAME } from "../../lib/brand.ts";
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
        message: `This dashboard needs ${BRAND_NAME} CLI web API ${WEB_COMPAT_API}, but the running gateway does not report web compatibility. Please update ${BRAND_NAME} CLI.`,
      };
    }
    if (gateway.api < WEB_COMPAT_API) {
      return {
        ok: false,
        message: `This dashboard needs ${BRAND_NAME} CLI web API ${WEB_COMPAT_API}; running gateway ${status.value.version} provides API ${gateway.api}. Please update ${BRAND_NAME} CLI.`,
      };
    }
    return { ok: true, message: "" };
  });
}
