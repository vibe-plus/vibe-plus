/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Dev: must match `vibe-core` listen port (`VITE_VIBE_PORT` in `.env.local`). */
  readonly VITE_VIBE_PORT?: string;
}
