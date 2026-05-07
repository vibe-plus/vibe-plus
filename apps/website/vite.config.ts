import { defineConfig } from "vite-plus";
import vue from "@vitejs/plugin-vue";
import UnoCSS from "unocss/vite";

export default defineConfig({
  plugins: [vue(), UnoCSS()],
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
