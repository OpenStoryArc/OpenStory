import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
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
      "/hooks": process.env.VITE_API_URL || "http://localhost:3002",
    },
  },
});
