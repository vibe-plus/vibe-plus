import type { ComputedRef, InjectionKey } from "vue";

export interface TabsContext {
  active: ComputedRef<string>;
  setActive: (value: string) => void;
}

export const TABS_CONTEXT: InjectionKey<TabsContext> = Symbol("vibe.tabs");
