import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // esbuild 0.28 (pulled in via the security override for GHSA-gv7w-rqvm-qjhr)
  // no longer down-transpiles modern syntax like destructuring to old targets —
  // it errors instead. A modern target means there's nothing to lower, which is
  // fine for a localhost dashboard.
  //   - build.target covers the production bundle.
  //   - optimizeDeps.esbuildOptions.target covers DEV dependency pre-bundling;
  //     without it the optimizer defaults to old browsers (chrome87/es2020) and
  //     esbuild 0.28 fatally fails pre-bundling deps that use destructuring
  //     (@tanstack/react-virtual, react-markdown), leaving a blank page.
  build: { target: "es2022" },
  optimizeDeps: { esbuildOptions: { target: "es2022" } },
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": process.env.VITE_API_URL || "http://localhost:3002",
      "/ws": {
        target: process.env.VITE_API_URL || "http://localhost:3002",
        ws: true,
      },
    },
  },
});
