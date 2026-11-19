import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// The Tauri shell loads ../dist/index.html; a fixed port keeps that path stable.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true, target: "es2021" },
});
