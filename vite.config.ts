import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/.tools/**", "**/src-tauri/**", "**/artifacts/**"] },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: { target: "es2022" },
});
