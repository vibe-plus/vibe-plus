/** 与 `router` 的 `name` 对齐；供各页标题/主按钮/标签使用（Tailwind 完整类名，避免动态拼接被 purge）。 */
export interface PageAccentClasses {
  /** 页面主标题 */
  heading: string;
  /** 小标签 / kicker */
  kicker: string;
  /** 主按钮（实心） */
  btnPrimary: string;
  /** 选中态 tab / chip */
  chipActive: string;
  /** 左侧竖条或 ring 点缀 */
  accentBar: string;
}

const fallback: PageAccentClasses = {
  heading: "text-violet-700",
  kicker: "text-violet-600 font-mono tracking-[0.12em]",
  btnPrimary:
    "bg-violet-600 text-white hover:bg-violet-700 shadow-md shadow-violet-600/20 focus-visible:ring-2 focus-visible:ring-violet-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
  chipActive: "bg-violet-600 text-white shadow-md shadow-violet-600/25",
  accentBar: "from-violet-500 to-violet-600",
};

export const pageAccentByRouteName: Record<string, PageAccentClasses> = {
  dashboard: {
    heading: "text-violet-700",
    kicker: "text-violet-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-violet-600 text-white hover:bg-violet-700 shadow-md shadow-violet-600/20 focus-visible:ring-2 focus-visible:ring-violet-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-violet-600 text-white shadow-md shadow-violet-600/25",
    accentBar: "from-violet-500 to-fuchsia-600",
  },
  providers: {
    heading: "text-teal-700",
    kicker: "text-teal-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-teal-600 text-white hover:bg-teal-700 shadow-md shadow-teal-600/20 focus-visible:ring-2 focus-visible:ring-teal-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-teal-600 text-white shadow-md shadow-teal-600/25",
    accentBar: "from-teal-500 to-cyan-600",
  },
  routes: {
    heading: "text-indigo-700",
    kicker: "text-indigo-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-indigo-600 text-white hover:bg-indigo-700 shadow-md shadow-indigo-600/20 focus-visible:ring-2 focus-visible:ring-indigo-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-indigo-600 text-white shadow-md shadow-indigo-600/25",
    accentBar: "from-indigo-500 to-blue-600",
  },
  logs: {
    heading: "text-amber-800",
    kicker: "text-amber-700 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-amber-600 text-white hover:bg-amber-700 shadow-md shadow-amber-600/20 focus-visible:ring-2 focus-visible:ring-amber-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-amber-600 text-white shadow-md shadow-amber-600/25",
    accentBar: "from-amber-500 to-orange-600",
  },
  usage: {
    heading: "text-emerald-700",
    kicker: "text-emerald-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-emerald-600 text-white hover:bg-emerald-700 shadow-md shadow-emerald-600/20 focus-visible:ring-2 focus-visible:ring-emerald-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-emerald-600 text-white shadow-md shadow-emerald-600/25",
    accentBar: "from-emerald-500 to-teal-600",
  },
  settings: {
    heading: "text-slate-700",
    kicker: "text-slate-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-slate-700 text-white hover:bg-slate-800 shadow-md shadow-slate-600/15 focus-visible:ring-2 focus-visible:ring-slate-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-slate-700 text-white shadow-md shadow-slate-600/20",
    accentBar: "from-slate-500 to-slate-700",
  },
};

export function resolvePageAccent(
  routeName: string | symbol | undefined | null,
): PageAccentClasses {
  const key = routeName == null ? "" : String(routeName);
  return pageAccentByRouteName[key] ?? fallback;
}

/** 供 UnoCSS `safelist` 使用：配色类仅在 TS 常量中，避免按需扫描遗漏导致运行时无样式 */
export function getPageAccentSafelistTokens(): string[] {
  const buckets: PageAccentClasses[] = [fallback, ...Object.values(pageAccentByRouteName)];
  const out = new Set<string>();
  for (const b of buckets) {
    for (const raw of [b.heading, b.kicker, b.btnPrimary, b.chipActive, b.accentBar]) {
      for (const t of raw.split(/\s+/)) {
        if (t) out.add(t);
      }
    }
  }
  return [...out];
}
