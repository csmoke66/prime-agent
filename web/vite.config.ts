import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// `npm run dev` proxies the backend started by `beehive web --port 7878`.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: { proxy: { "/api": "http://127.0.0.1:7878", "/ws": { target: "ws://127.0.0.1:7878", ws: true } } },
  build: { outDir: "../crates/bh-web/assets", emptyOutDir: true, chunkSizeWarningLimit: 1500 },
});
