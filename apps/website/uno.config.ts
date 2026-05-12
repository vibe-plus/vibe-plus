import { defineConfig, presetIcons, presetUno, presetWebFonts } from "unocss";
import { getPageAccentSafelistTokens } from "./src/utils/page-accent.ts";
import { lobeIconsCollection, vibePlusIconsCollection } from "./uno-icons.ts";

export default defineConfig({
  safelist: getPageAccentSafelistTokens(),
  presets: [
    presetUno(),
    presetIcons({
      collections: {
        lucide: () => import("@iconify-json/lucide/icons.json").then((i) => i.default),
        lobe: lobeIconsCollection,
        vp: vibePlusIconsCollection,
      },
      extraProperties: {
        display: "inline-block",
        "vertical-align": "middle",
      },
    }),
    presetWebFonts({
      provider: "none",
      fonts: {
        sans: ['"Inter"', '"SF Pro"', "system-ui", "sans-serif"],
        mono: ['"JetBrains Mono"', '"SF Mono"', "Fira Code", "monospace"],
      },
    }),
  ],
  shortcuts: {
    "layout-shell":
      "min-h-screen flex flex-col sm:flex-row antialiased font-sans bg-vp-bg text-vp-text",
    "nav-aside":
      "shrink-0 border-b sm:border-b-0 sm:border-r border-vp-border bg-vp-surface flex sm:flex-col relative shadow-sm",
    "nav-link":
      "flex items-center justify-center sm:justify-start gap-2 sm:gap-3 px-3 py-2 sm:px-3.5 sm:py-2.5 rounded-xl text-[13px] font-medium transition-all duration-200",
    "nav-link--idle":
      "text-vp-muted hover:text-vp-text hover:bg-[color-mix(in_srgb,var(--vp-text)_5%,var(--vp-surface))]",
    "nav-link--active":
      "bg-[color-mix(in_srgb,var(--vp-primary)_12%,var(--vp-surface))] text-[color-mix(in_srgb,var(--vp-primary)_34%,var(--vp-text))] shadow-sm",
    "nav-icon": "size-7 rounded-lg flex items-center justify-center transition-colors duration-200",
    "nav-icon--idle": "text-vp-muted group-hover:text-vp-text",
    "nav-icon--active": "text-[color-mix(in_srgb,var(--vp-primary)_18%,var(--vp-text))]",
    "top-tabs":
      "sticky top-0 z-20 flex items-center gap-1 overflow-x-auto rounded-xl border border-vp-border bg-[color-mix(in_srgb,var(--vp-surface)_92%,white)] p-1 shadow-sm backdrop-blur",
    "top-tab":
      "inline-flex h-9 min-w-9 shrink-0 items-center justify-center gap-1.5 rounded-lg px-2 sm:px-3 text-xs font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]",
    "top-tab--idle": "text-vp-muted hover:bg-vp-surface hover:text-vp-text",
    "top-tab--active": "bg-vp-surface text-vp-text shadow-sm ring-1 ring-vp-border",
    "main-canvas": "flex-1 min-w-0 overflow-auto bg-vp-bg relative",
    "brand-mark":
      "size-8 rounded-xl bg-gradient-to-br from-vp-primary to-[color-mix(in_srgb,var(--vp-primary)_65%,#1e1b4b)] flex items-center justify-center text-white text-sm font-bold shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]",
    "btn-primary":
      "px-4 py-2 rounded-lg text-sm font-medium bg-vp-primary text-white hover:brightness-95 active:brightness-90 transition-all duration-200 shadow-md shadow-[color-mix(in_srgb,var(--vp-primary)_28%,transparent)] active:scale-[0.98]",
    "btn-ghost":
      "px-4 py-2 rounded-lg text-sm font-medium text-vp-muted hover:text-vp-text hover:bg-[color-mix(in_srgb,var(--vp-text)_6%,var(--vp-surface))] transition-all duration-200 active:scale-[0.98]",
    "card-base":
      "rounded-xl border border-vp-border bg-vp-surface shadow-sm transition-all duration-200 hover:border-[color-mix(in_srgb,var(--vp-text)_14%,var(--vp-border))]",
    "input-base":
      "w-full bg-vp-surface border border-vp-border rounded-lg px-3 py-2 text-sm text-vp-text placeholder-vp-muted focus:outline-none focus:border-[color-mix(in_srgb,var(--vp-primary)_45%,var(--vp-border))] focus:ring-1 focus:ring-[color-mix(in_srgb,var(--vp-primary)_22%,transparent)] transition-all duration-200",
    "label-base": "text-xs font-medium text-vp-muted tracking-wide",
    "badge-green":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-emerald-200 bg-emerald-50 text-emerald-800",
    "badge-amber":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-amber-200 bg-amber-50 text-amber-900",
    "badge-red":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-red-200 bg-red-50 text-red-800",
    "badge-purple":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-indigo-200 bg-indigo-50 text-indigo-800",
    "stat-value": "text-xl sm:text-2xl font-bold text-vp-text tracking-tight",
    "stat-label": "text-xs text-vp-muted uppercase tracking-wide",
    /** 主内容区：柔和浅色表面（嵌在浅色 layout 内） */
    "page-surface":
      "min-w-0 rounded-2xl sm:rounded-3xl border border-vp-border bg-[color-mix(in_srgb,var(--vp-surface)_96%,white)] text-vp-text shadow-[0_10px_44px_color-mix(in_srgb,var(--vp-text)_7%,transparent)]",
    "vp-modal-backdrop":
      "fixed inset-0 z-[100] flex items-center justify-center p-3 sm:p-6 bg-[color-mix(in_srgb,var(--vp-text)_42%,transparent)] backdrop-blur-md",
    "vp-modal-panel":
      "w-full max-h-[90vh] flex flex-col overflow-hidden rounded-2xl border border-vp-border bg-vp-surface text-vp-text shadow-2xl ring-1 ring-[color-mix(in_srgb,var(--vp-border)_80%,transparent)]",
    "vp-modal-header":
      "flex flex-wrap items-start gap-3 px-5 py-4 border-b border-vp-border shrink-0 bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]",
    /** 浅色弹层内图标按钮 */
    "vp-icon-btn":
      "inline-flex min-h-11 min-w-11 items-center justify-center rounded-xl p-2 text-vp-muted hover:text-vp-text hover:bg-[color-mix(in_srgb,var(--vp-text)_6%,var(--vp-surface))] border border-transparent hover:border-vp-border transition-colors disabled:opacity-40 disabled:pointer-events-none focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]",
  },
  theme: {
    colors: {
      vp: {
        bg: "var(--vp-bg)",
        surface: "var(--vp-surface)",
        text: "var(--vp-text)",
        muted: "var(--vp-muted)",
        primary: "var(--vp-primary)",
        border: "var(--vp-border)",
        danger: "var(--vp-danger)",
      },
    },
  },
});
