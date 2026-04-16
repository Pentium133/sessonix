import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Dogfooding: when Sessonix creates a task worktree inside the aicoder
      // repo itself, Vite would otherwise see the new files and full-reload,
      // resetting UI state (active project, active session, etc.).
      ignored: ["**/src-tauri/**", "**/.sessonix-worktrees/**"],
    },
  },
});
