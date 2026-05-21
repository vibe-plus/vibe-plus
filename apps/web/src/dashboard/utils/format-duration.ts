/** Human-readable duration from milliseconds (ms / s / m / h). */
export function formatDurationMs(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";

  if (ms < 1000) {
    return `${Math.round(ms)}ms`;
  }

  const sec = ms / 1000;
  if (sec < 60) {
    const digits = sec < 10 ? 2 : 1;
    const raw = sec.toFixed(digits);
    const trimmed = raw.replace(/(\.\d*?)0+$/, "$1").replace(/\.$/, "");
    return `${trimmed}s`;
  }

  const totalSec = Math.round(sec);
  const minutes = Math.floor(totalSec / 60);
  const seconds = totalSec % 60;
  if (minutes < 60) {
    return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
  }

  const hours = Math.floor(minutes / 60);
  const remMinutes = minutes % 60;
  return remMinutes > 0 ? `${hours}h ${remMinutes}m` : `${hours}h`;
}
