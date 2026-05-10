import { defineConfig, presetUno, presetWebFonts } from "unocss";

export default defineConfig({
  presets: [
    presetUno(),
    presetWebFonts({
      provider: "none",
      fonts: {
        sans: ['"Inter"', '"SF Pro"', "system-ui", "sans-serif"],
        mono: ['"JetBrains Mono"', '"SF Mono"', "Fira Code", "monospace"],
      },
    }),
  ],
  shortcuts: {
    "btn-primary":
      "px-4 py-2 rounded-lg text-sm font-medium bg-violet-600 text-white hover:bg-violet-500 transition-all duration-200 shadow-lg shadow-violet-900/30 hover:shadow-violet-900/50 active:scale-[0.98]",
    "btn-ghost":
      "px-4 py-2 rounded-lg text-sm font-medium text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/80 transition-all duration-200 active:scale-[0.98]",
    "card-base":
      "rounded-xl border border-white/[0.06] bg-[#1a1a1f] transition-all duration-200 hover:border-white/[0.12]",
    "input-base":
      "w-full bg-zinc-800/50 border border-white/[0.08] rounded-lg px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:outline-none focus:border-violet-500/50 focus:ring-1 focus:ring-violet-500/30 transition-all duration-200",
    "label-base": "text-xs font-medium text-zinc-400 tracking-wide",
    "badge-green":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-emerald-500/30 bg-emerald-500/15 text-emerald-300",
    "badge-amber":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-amber-500/30 bg-amber-500/15 text-amber-300",
    "badge-red":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-red-500/30 bg-red-500/15 text-red-300",
    "badge-purple":
      "text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border border-violet-500/30 bg-violet-500/15 text-violet-300",
    "stat-value": "text-2xl sm:text-3xl font-bold text-white tracking-tight",
    "stat-label": "text-xs text-zinc-500 uppercase tracking-wide",
  },
  theme: {
    colors: {
      vp: {
        brand: "#8b5cf6",
        "brand-light": "#a78bfa",
        "brand-dark": "#7c3aed",
        accent: "#22d3ee",
      },
    },
  },
});
