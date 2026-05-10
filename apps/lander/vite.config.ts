import { defineConfig } from "vite-plus";
import vue from "@vitejs/plugin-vue";
import UnoCSS from "unocss/vite";
import singleton from "unplugin-singleton/vite";
import caddyLocalhost from "unplugin-caddy-localhost/vite";

export default defineConfig({
  plugins: [caddyLocalhost(), singleton(), vue(), UnoCSS()],
  build: {
    outDir: "dist",
  },
  lint: {
    options: {
      typeAware: false,
    },
  },
  fmt: {},
});
