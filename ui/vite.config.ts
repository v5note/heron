import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// The Tauri shell loads ../dist/index.html; a fixed port keeps that path stable.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2021",
    // Brand art (the two rail logos) is hundreds of KB: never let it become a base64 blob inside
    // the entry chunk, where it would be parsed before anything renders. Emit it as a file.
    assetsInlineLimit: (file) => (/logo.*\.png$/.test(file) ? false : undefined),
  },
});
