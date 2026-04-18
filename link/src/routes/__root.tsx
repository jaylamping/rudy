import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import { WebTransportBridge } from "@/lib/hooks/WebTransportBridge";

interface RouterContext {
  queryClient: QueryClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
});

function RootLayout() {
  // The bridge is rendered exactly once for the lifetime of the SPA. It owns
  // the single QUIC session and pushes telemetry into the TanStack Query
  // cache so leaf components can keep using `useQuery` without changes. See
  // `link/src/lib/hooks/WebTransportBridge.tsx` for the design notes.
  return (
    <>
      <WebTransportBridge />
      <Outlet />
    </>
  );
}
