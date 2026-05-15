import { readFileSync } from "node:fs";
import { defineConfig } from "vite-plus";
import vue from "@vitejs/plugin-vue";
import UnoCSS from "unocss/vite";
import singleton from "unplugin-singleton/vite";
import caddyLocalhost from "unplugin-caddy-localhost/vite";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as {
  version: string;
};

export default defineConfig({
  base: process.env.VITE_BASE_PATH ?? "/",
  plugins: [caddyLocalhost(), singleton(), vue(), UnoCSS()],
  build: {
    outDir: "dist",
  },
  define: {
    "import.meta.env.VITE_UI_VERSION": JSON.stringify(pkg.version),
  },
  lint: {
    options: {
      typeAware: false,
    },
  },
  fmt: {},
});
