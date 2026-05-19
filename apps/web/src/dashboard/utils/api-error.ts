import type { ComposerTranslation } from "vue-i18n";
import { ApiError } from "../api/client.ts";

function looksLikeCircuitBreakerError(error: ApiError): boolean {
  if (error.status !== 503) return false;
  const text = `${error.bodyText} ${error.message}`.toLowerCase();
  return text.includes("circuit breaker") || text.includes("providers blocked");
}

function parseApiErrorBody(bodyText: string): string | null {
  const trimmed = bodyText.trim();
  if (!trimmed) return null;
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (typeof parsed === "string") return parsed;
    if (parsed && typeof parsed === "object") {
      const maybeMessage = (parsed as Record<string, unknown>).message;
      if (typeof maybeMessage === "string" && maybeMessage.trim()) return maybeMessage.trim();
      const maybeDetail = (parsed as Record<string, unknown>).detail;
      if (typeof maybeDetail === "string" && maybeDetail.trim()) return maybeDetail.trim();
    }
  } catch {
    // Fall through to raw text.
  }
  return trimmed;
}

export function formatApiError(error: unknown, t: ComposerTranslation): string {
  if (error instanceof ApiError) {
    if (looksLikeCircuitBreakerError(error)) {
      return t("errors.circuitBreakerBlocked");
    }
    const parsed = parseApiErrorBody(error.bodyText);
    if (parsed) return parsed;
    return t("errors.requestFailed", { status: error.status });
  }

  if (error instanceof Error) {
    const message = error.message.trim();
    if (!message) return t("errors.requestFailed", { status: "?" });
    return message;
  }

  const fallback = String(error).trim();
  return fallback || t("errors.requestFailed", { status: "?" });
}
