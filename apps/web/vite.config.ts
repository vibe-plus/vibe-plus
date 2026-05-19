import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { resolve, relative, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";
import { defineConfig } from "vite-plus";
import vue from "@vitejs/plugin-vue";
import VueI18nPlugin from "@intlify/unplugin-vue-i18n/vite";
import tailwindcss from "@tailwindcss/vite";
import singleton from "unplugin-singleton/vite";
import caddyLocalhost from "unplugin-caddy-localhost/vite";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as {
  version: string;
};

const __dirname = fileURLToPath(new URL(".", import.meta.url));

/**
 * Write `dist/version.json` after every production build.
 * The desktop app reads this to know the embedded UI version and to build
 * the CDN download manifest for the background updater.
 *
 * Format:
 *   { "version": "0.1.2", "min_cli_protocol": 1, "files": ["index.html", "assets/…"] }
 *
 * `min_cli_protocol` must be bumped whenever the UI starts relying on a new
 * `/_vp/` gateway endpoint that older CLI binaries don't provide.
 */
function generateUiManifest(version: string, minCliProtocol = 1): Plugin {
  return {
    name: "vibe-ui-manifest",
    apply: "build",
    closeBundle() {
      const distDir = resolve(__dirname, "dist");

      function collectFiles(dir: string): string[] {
        const results: string[] = [];
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
          const full = join(dir, entry.name);
          const rel = relative(distDir, full).replace(/\\/g, "/");
          if (entry.isDirectory()) {
            results.push(...collectFiles(full));
          } else if (rel !== "version.json") {
            results.push(rel);
          }
        }
        return results;
      }

      let files: string[] = [];
      try {
        files = collectFiles(distDir);
      } catch {
        // dist not yet populated (e.g. incremental watch); skip silently
        return;
      }

      const manifest = { version, min_cli_protocol: minCliProtocol, files };
      writeFileSync(resolve(distDir, "version.json"), JSON.stringify(manifest, null, 2) + "\n");
      console.log(
        `[vibe-ui-manifest] wrote dist/version.json (v${version}, ${files.length} files)`,
      );
    },
  };
}

export default defineConfig({
  base: process.env.VITE_BASE_PATH ?? "/",
  server: {
    port: 15876,
    strictPort: true,
  },
  plugins: [
    caddyLocalhost(),
    ...(process.env.NODE_ENV === "test" || process.env.VITEST ? [] : [singleton()]),
    vue(),
    VueI18nPlugin({
      runtimeOnly: false,
      compositionOnly: true,
      defaultSFCLang: "json",
    }),
    tailwindcss(),
    generateUiManifest(pkg.version),
  ],
  build: {
    outDir: "dist",
  },
  define: {
    "import.meta.env.VITE_UI_VERSION": JSON.stringify(pkg.version),
  },
  lint: {
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
  fmt: {},
});
