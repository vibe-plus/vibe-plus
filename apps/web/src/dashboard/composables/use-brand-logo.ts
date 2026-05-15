import { computed, readonly, shallowRef, watch } from "vue";
import softMintLogo from "../assets/brand/vibe-plus-icon-soft-mint.svg";
import softSkyLogo from "../assets/brand/vibe-plus-icon-soft-sky.svg";
import coralLogo from "../assets/brand/vibe-plus-icon-coral.svg";
import lilacLogo from "../assets/brand/vibe-plus-icon-lilac.svg";
import leafLogo from "../assets/brand/vibe-plus-icon-leaf.svg";

const STORAGE_KEY = "vibe-plus:brand-logo";
const DEFAULT_LOGO_ID = "soft-mint";

interface BrandTheme {
  primary: string;
  brandLight: string;
  brandDark: string;
  bg: string;
  surface: string;
  border: string;
}

export const BRAND_LOGOS = [
  {
    id: "soft-mint",
    label: "Mint",
    src: softMintLogo,
    accent: "#73d9ae",
    theme: {
      primary: "#2cc09e",
      brandLight: "#73d9ae",
      brandDark: "#1a9b7e",
      bg: "#e4edea",
      surface: "#f5faf8",
      border: "rgba(0, 100, 70, 0.10)",
    } satisfies BrandTheme,
  },
  {
    id: "soft-sky",
    label: "Sky",
    src: softSkyLogo,
    accent: "#77bee9",
    theme: {
      primary: "#3da5d9",
      brandLight: "#77bee9",
      brandDark: "#1f85c0",
      bg: "#e4eaf0",
      surface: "#f4f8fc",
      border: "rgba(0, 80, 140, 0.10)",
    } satisfies BrandTheme,
  },
  {
    id: "coral",
    label: "Coral",
    src: coralLogo,
    accent: "#ff8a78",
    theme: {
      primary: "#f0614d",
      brandLight: "#ff8a78",
      brandDark: "#d44132",
      bg: "#f0e9e6",
      surface: "#fdf5f3",
      border: "rgba(160, 50, 30, 0.10)",
    } satisfies BrandTheme,
  },
  {
    id: "lilac",
    label: "Lilac",
    src: lilacLogo,
    accent: "#b8a7ff",
    theme: {
      primary: "#7c65f5",
      brandLight: "#b8a7ff",
      brandDark: "#5c47e0",
      bg: "#eceaf2",
      surface: "#f7f5fb",
      border: "rgba(80, 50, 180, 0.10)",
    } satisfies BrandTheme,
  },
  {
    id: "leaf",
    label: "Leaf",
    src: leafLogo,
    accent: "#93d487",
    theme: {
      primary: "#4db946",
      brandLight: "#93d487",
      brandDark: "#35913e",
      bg: "#e5edea",
      surface: "#f4faf5",
      border: "rgba(20, 100, 30, 0.10)",
    } satisfies BrandTheme,
  },
] as const;

export type BrandLogoId = (typeof BRAND_LOGOS)[number]["id"];

function isBrandLogoId(value: string | null): value is BrandLogoId {
  return BRAND_LOGOS.some((logo) => logo.id === value);
}

function readStoredLogoId(): BrandLogoId {
  if (typeof window === "undefined") return DEFAULT_LOGO_ID;
  const stored = window.localStorage.getItem(STORAGE_KEY);
  return isBrandLogoId(stored) ? stored : DEFAULT_LOGO_ID;
}

function writeStoredLogoId(id: BrandLogoId) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(STORAGE_KEY, id);
}

function syncFavicon(src: string) {
  if (typeof document === "undefined") return;
  let link = document.querySelector<HTMLLinkElement>('link[rel="icon"][data-vp-brand="true"]');
  link ??= document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  if (!link) {
    link = document.createElement("link");
    document.head.appendChild(link);
  }
  link.rel = "icon";
  link.type = "image/svg+xml";
  link.href = src;
  link.dataset.vpBrand = "true";
}

function syncThemeVars(theme: BrandTheme) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.style.setProperty("--vp-primary", theme.primary);
  root.style.setProperty("--vp-brand-light", theme.brandLight);
  root.style.setProperty("--vp-brand-dark", theme.brandDark);
  root.style.setProperty("--vp-accent", theme.primary);
  root.style.setProperty("--vp-bg", theme.bg);
  root.style.setProperty("--vp-surface", theme.surface);
  root.style.setProperty("--vp-border", theme.border);
}

const selectedBrandLogoId = shallowRef<BrandLogoId>(readStoredLogoId());
const brandLogos = BRAND_LOGOS;
const currentBrandLogo = computed(
  () => brandLogos.find((logo) => logo.id === selectedBrandLogoId.value) ?? brandLogos[0],
);
let syncStarted = false;

function ensureBrandLogoSync() {
  if (syncStarted) return;
  syncStarted = true;
  watch(
    currentBrandLogo,
    (logo) => {
      syncFavicon(logo.src);
      syncThemeVars(logo.theme);
    },
    { immediate: true },
  );

  if (typeof window === "undefined") return;
  window.addEventListener("storage", (event) => {
    if (event.key !== STORAGE_KEY || !isBrandLogoId(event.newValue)) return;
    selectedBrandLogoId.value = event.newValue;
  });
}

export function setBrandLogo(id: BrandLogoId) {
  selectedBrandLogoId.value = id;
  writeStoredLogoId(id);
}

export function useBrandLogo() {
  ensureBrandLogoSync();
  return {
    brandLogos,
    currentBrandLogo,
    selectedBrandLogoId: readonly(selectedBrandLogoId),
    setBrandLogo,
  };
}
