import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Ignore the Rust side so HMR doesn't loop on cargo
      // build artifacts.
      ignored: ["**/src-tauri/**"],
    },
  },
});
