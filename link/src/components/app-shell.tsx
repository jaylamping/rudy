import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { Activity, Cog, Gamepad2, ScrollText, View } from "lucide-react";
import { cn } from "@/lib/utils";

const NAV = [
  { to: "/telemetry", label: "Telemetry", Icon: Activity },
  { to: "/params", label: "Params", Icon: Cog },
  { to: "/jog", label: "Jog", Icon: Gamepad2 },
  { to: "/viz", label: "Viz", Icon: View },
  { to: "/logs", label: "Logs", Icon: ScrollText },
] as const;

export function AppShell() {
  const { location } = useRouterState();

  return (
    <div className="flex h-full min-h-screen">
      <aside className="flex w-56 shrink-0 flex-col border-r border-border bg-card">
        <div className="border-b border-border px-4 py-4">
          <div className="text-sm font-semibold">Rudy</div>
          <div className="text-xs text-muted-foreground">operator console</div>
        </div>
        <nav className="flex-1 space-y-0.5 p-2">
          {NAV.map(({ to, label, Icon }) => {
            const active = location.pathname.startsWith(to);
            return (
              <Link
                key={to}
                to={to}
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
        <div className="mx-auto max-w-6xl p-6">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
