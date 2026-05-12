<script setup lang="ts">
import { computed } from "vue";

export type vp_icon_name =
  | "layout-dashboard"
  | "server"
  | "route"
  | "file-text"
  | "pie-chart"
  | "settings"
  | "refresh-cw"
  | "x"
  | "copy"
  | "download"
  | "save"
  | "trash-2"
  | "sparkles"
  | "plus"
  | "key"
  | "pencil"
  | "rotate-ccw"
  | "folder-input"
  | "clipboard"
  | "upload"
  | "shield"
  | "power"
  | "plug"
  | "terminal"
  | "file-code"
  | "activity"
  | "zap"
  | "loader-2"
  | "check"
  | "chevron-down"
  | "pause"
  | "alert-triangle"
  | "star"
  | "circle"
  | "bot"
  | "brain"
  | "cloud"
  | "moon"
  | "gem"
  | "package"
  | "archive"
  | "compass"
  | "book-open"
  | "terminal-square";

const props = defineProps<{
  name: vp_icon_name;
  /** Tailwind / Uno size class，默认 size-5 */
  sizeClass?: string;
  /** 为刷新等图标启用旋转动画 */
  spin?: boolean;
}>();

const iconClassByName: Record<vp_icon_name, string> = {
  "layout-dashboard": "i-lucide-layout-dashboard",
  server: "i-lucide-server",
  route: "i-lucide-route",
  "file-text": "i-lucide-file-text",
  "pie-chart": "i-lucide-pie-chart",
  settings: "i-lucide-settings",
  "refresh-cw": "i-lucide-refresh-cw",
  x: "i-lucide-x",
  copy: "i-lucide-copy",
  download: "i-lucide-download",
  save: "i-lucide-save",
  "trash-2": "i-lucide-trash-2",
  sparkles: "i-lucide-sparkles",
  plus: "i-lucide-plus",
  key: "i-lucide-key",
  pencil: "i-lucide-pencil",
  "rotate-ccw": "i-lucide-rotate-ccw",
  "folder-input": "i-lucide-folder-input",
  clipboard: "i-lucide-clipboard",
  upload: "i-lucide-upload",
  shield: "i-lucide-shield",
  power: "i-lucide-power",
  plug: "i-lucide-plug",
  terminal: "i-lucide-terminal",
  "file-code": "i-lucide-file-code",
  activity: "i-lucide-activity",
  zap: "i-lucide-zap",
  "loader-2": "i-lucide-loader-2",
  check: "i-lucide-check",
  "chevron-down": "i-lucide-chevron-down",
  pause: "i-lucide-pause",
  "alert-triangle": "i-lucide-alert-triangle",
  star: "i-lucide-star",
  circle: "i-lucide-circle",
  bot: "i-lucide-bot",
  brain: "i-lucide-brain",
  cloud: "i-lucide-cloud",
  moon: "i-lucide-moon",
  gem: "i-lucide-gem",
  package: "i-lucide-package",
  archive: "i-lucide-archive",
  compass: "i-lucide-compass",
  "book-open": "i-lucide-book-open",
  "terminal-square": "i-lucide-square-terminal",
};

const iconClass = computed(() => iconClassByName[props.name]);
</script>

<template>
  <span
    :class="[iconClass, sizeClass ?? 'size-5 shrink-0', spin ? 'animate-spin' : '']"
    aria-hidden="true"
  />
</template>
