import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// During `npm run dev` the SPA is served by Vite, but `/api/*` calls are
// proxied to a real `rudydae`. By default we point at a local daemon
// (`127.0.0.1:8443`). To talk to the Rudy Pi over Tailscale instead, set
// VITE_RUDYD_URL in `link/.env.local` (or your shell), e.g.
//   VITE_RUDYD_URL=https://rudy.your-tailnet.ts.net:8443
// The Pi's cert is a real Tailscale-issued LetsEncrypt cert, so no extra
// `secure: false` gymnastics are required — but we leave it off anyway so
// self-signed dev certs also work.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "VITE_");
  const target = env.VITE_RUDYD_URL ?? "http://127.0.0.1:8443";

  return {
    plugins: [
      tanstackRouter({
        routesDirectory: "src/routes",
        generatedRouteTree: "src/routeTree.gen.ts",
      }),
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
          target,
          changeOrigin: true,
          secure: false,
        },
      },
    },
    build: {
      outDir: "dist",
      sourcemap: true,
    },
  };
});
