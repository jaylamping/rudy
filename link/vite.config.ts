import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// `link` is served three ways:
//   - `npm run dev`       -> Vite on :5173, proxies /api -> rudyd
//   - `npm run build`     -> static dist/, embedded into rudyd at cargo build
//   - offsite laptop      -> `VITE_RUDYD_URL=https://rudy.ts.net:8443 npm run dev`
//     (no proxy; absolute URLs with CORS handled by rudyd)
export default defineConfig({
  plugins: [
    TanStackRouterVite({ routesDirectory: "src/routes", generatedRouteTree: "src/routeTree.gen.ts" }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.VITE_RUDYD_URL ?? "http://127.0.0.1:8443",
        changeOrigin: true,
        secure: false,
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
