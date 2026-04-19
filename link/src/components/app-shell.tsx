import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import {
  Activity,
  Cog,
  Gamepad2,
  LayoutDashboard,
  ScrollText,
  View,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { EstopButton } from "@/components/estop-button";
import { LimbQuarantineToaster } from "@/components/limb-quarantine-toaster";

// `/` resolves to `_app.index.tsx` (the Overview dashboard). All the
// other entries are siblings of the Overview route and render inside the
// same shell.
interface NavItem {
  to: string;
  label: string;
  Icon: typeof Activity;
  exact?: boolean;
}

const NAV: readonly NavItem[] = [
  { to: "/", label: "Overview", Icon: LayoutDashboard, exact: true },
  { to: "/telemetry", label: "Telemetry", Icon: Activity },
  { to: "/params", label: "Params", Icon: Cog },
  { to: "/jog", label: "Jog", Icon: Gamepad2 },
  { to: "/viz", label: "Viz", Icon: View },
  { to: "/logs", label: "Logs", Icon: ScrollText },
];

export function AppShell() {
  const { location } = useRouterState();

  return (
    <div className="flex h-full min-h-screen">
      <aside className="flex w-56 shrink-0 flex-col border-r border-border bg-card">
        <div className="border-b border-border px-4 py-4">
          <div className="text-lg font-semibold">Rudy</div>
          <div className="text-xs text-muted-foreground pt-2">console</div>
        </div>
        <nav className="flex-1 space-y-0.5 p-2">
          {NAV.map(({ to, label, Icon, exact }) => {
            const active = exact
              ? location.pathname === to
              : location.pathname.startsWith(to);
            return (
              <Link
                key={to}
                to={to as "/"}
                className={cn(
                  "flex items-center gap-2 rounded-md px-3 py-2 text-sm transition",
                  active
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                <Icon className="h-4 w-4" />
                <span>{label}</span>
              </Link>
            );
          })}
        </nav>
      </aside>
      <main className="flex-1 overflow-auto bg-background">
        <header className="sticky top-0 z-40 flex items-center justify-end gap-4 border-b border-border bg-background/85 px-6 py-3 backdrop-blur">
          <EstopButton />
        </header>
        <div className="mx-auto max-w-6xl p-6">
          <Outlet />
        </div>
      </main>
      <LimbQuarantineToaster />
    </div>
  );
}
