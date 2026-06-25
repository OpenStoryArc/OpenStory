import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // esbuild 0.28 (pulled in via the security override for GHSA-gv7w-rqvm-qjhr)
  // no longer down-transpiles modern syntax like destructuring to old targets.
  // A modern build target means there's nothing to lower — fine for a
  // localhost dashboard. (Dev/test transforms already default to esnext.)
  build: { target: "es2022" },
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
