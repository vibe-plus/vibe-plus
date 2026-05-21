import { ref, watch } from "vue";

const STORAGE_KEY = "vp.observability.privacy";

function read(): boolean {
  if (typeof window === "undefined") return true;
  const raw = window.localStorage.getItem(STORAGE_KEY);
  if (raw === null) return true; // privacy-on by default
  return raw === "1";
}

const privacy = ref<boolean>(read());

if (typeof window !== "undefined") {
  watch(
    privacy,
    (next) => {
      window.localStorage.setItem(STORAGE_KEY, next ? "1" : "0");
    },
    { flush: "post" },
  );
}

export function usePrivacyMode() {
  function toggle() {
    privacy.value = !privacy.value;
  }
  function mask(text: string, fallback = "•••"): string {
    if (!privacy.value) return text;
    if (!text) return fallback;
    // Show first 2 and last 2 visible chars, mask the middle.
    const trimmed = text.trim();
    if (trimmed.length <= 4) return "•".repeat(Math.max(3, trimmed.length));
    return `${trimmed.slice(0, 2)}${"•".repeat(Math.min(6, trimmed.length - 4))}${trimmed.slice(-2)}`;
  }
  function maskPath(path: string): string {
    if (!privacy.value || !path) return path;
    // Keep base, mask middle.
    const parts = path.split("/").filter(Boolean);
    if (parts.length <= 1) return mask(path);
    return `…/${parts[parts.length - 1]}`;
  }
  return { privacy, toggle, mask, maskPath };
}
