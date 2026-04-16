import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      // Dogfooding: task worktrees inside the repo carry their own copy of src/
      // which would be collected twice without this.
      "**/.sessonix-worktrees/**",
    ],
  },
});
