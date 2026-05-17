/** Aligned with router `name`; used by page titles, primary buttons, and labels. Keep full Tailwind class names here so the scanner can detect them. */
export interface PageAccentClasses {
  /** Page main title */
  heading: string;
  /** Small label / kicker */
  kicker: string;
  /** Primary button (solid) */
  btnPrimary: string;
  /** Selected tab / chip */
  chipActive: string;
  /** Left rail or ring accent */
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
  overview: {
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
  logs: {
    heading: "text-amber-800",
    kicker: "text-amber-700 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-amber-600 text-white hover:bg-amber-700 shadow-md shadow-amber-600/20 focus-visible:ring-2 focus-visible:ring-amber-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-amber-600 text-white shadow-md shadow-amber-600/25",
    accentBar: "from-amber-500 to-orange-600",
  },
  statistics: {
    heading: "text-emerald-700",
    kicker: "text-emerald-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-emerald-600 text-white hover:bg-emerald-700 shadow-md shadow-emerald-600/20 focus-visible:ring-2 focus-visible:ring-emerald-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-emerald-600 text-white shadow-md shadow-emerald-600/25",
    accentBar: "from-emerald-500 to-teal-600",
  },
  monitor: {
    heading: "text-sky-700",
    kicker: "text-sky-600 font-mono tracking-[0.12em]",
    btnPrimary:
      "bg-sky-600 text-white hover:bg-sky-700 shadow-md shadow-sky-600/20 focus-visible:ring-2 focus-visible:ring-sky-500/45 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50",
    chipActive: "bg-sky-600 text-white shadow-md shadow-sky-600/25",
    accentBar: "from-sky-500 to-blue-600",
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
