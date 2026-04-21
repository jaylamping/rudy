import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import {
  Activity,
  Cog,
  Gamepad2,
  HardDrive,
  LayoutDashboard,
  Menu,
  ScrollText,
  View,
} from "lucide-react";
import { useState } from "react";
import { cn } from "@/lib/utils";
import { EstopButton } from "@/components/estop-button";
import { GlobalActuatorHealthBar } from "@/components/global-actuator-health-bar";
import { LimbQuarantineToaster } from "@/components/limb-quarantine-toaster";
import { RestartButton } from "@/components/restart-button";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

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
  { to: "/devices", label: "Devices", Icon: HardDrive },
  { to: "/telemetry", label: "Telemetry", Icon: Activity },
  { to: "/params", label: "Params", Icon: Cog },
  { to: "/jog", label: "Jog", Icon: Gamepad2 },
  { to: "/viz", label: "Viz", Icon: View },
  { to: "/logs", label: "Logs", Icon: ScrollText },
] as const;

function NavLinks({
  onNavigate,
  locationPathname,
}: {
  onNavigate?: () => void;
  locationPathname: string;
}) {
  return (
    <>
      {NAV.map(({ to, label, Icon, exact }) => {
        const active = exact ? locationPathname === to : locationPathname.startsWith(to);
        return (
          <Link
            key={to}
            to={to}
            onClick={onNavigate}
            className={cn(
              "flex items-center gap-2 rounded-md px-3 py-2 text-sm transition",
              active
                ? "bg-accent text-accent-foreground"
                : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
            )}
          >
            <Icon className="h-4 w-4 shrink-0" />
            <span>{label}</span>
          </Link>
        );
      })}
    </>
  );
}

export function AppShell() {
  const { location } = useRouterState();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  return (
    <div className="flex h-full min-h-screen flex-col md:flex-row">
      {/* Desktop sidebar */}
      <aside className="hidden w-56 shrink-0 flex-col border-r border-border bg-card md:flex">
        <div className="border-b border-border px-4 py-4">
          <div className="text-lg font-semibold">Rudy</div>
          <div className="pt-2 text-xs text-muted-foreground">console</div>
        </div>
        <nav className="flex-1 space-y-0.5 p-2">
          <NavLinks locationPathname={location.pathname} />
        </nav>
      </aside>

      {/* Mobile top bar + menu */}
      <div className="flex items-center gap-2 border-b border-border bg-card px-3 py-2 md:hidden">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="shrink-0"
          aria-label="Open menu"
          onClick={() => setMobileMenuOpen(true)}
        >
          <Menu className="h-5 w-5" />
        </Button>
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">Rudy console</div>
        </div>
      </div>

      <Dialog open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>Navigate</DialogTitle>
            <DialogDescription>Jump to a console page.</DialogDescription>
          </DialogHeader>
          <nav className="flex flex-col space-y-0.5">
            <NavLinks
              locationPathname={location.pathname}
              onNavigate={() => setMobileMenuOpen(false)}
            />
          </nav>
        </DialogContent>
      </Dialog>

      <main className="flex min-h-0 flex-1 flex-col overflow-auto bg-background">
        <header className="sticky top-0 z-40 flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-border bg-background/85 px-4 py-3 backdrop-blur sm:px-6">
          <div className="mr-auto min-w-0">
            <GlobalActuatorHealthBar />
          </div>
          <RestartButton />
          <EstopButton />
        </header>
        <div className="mx-auto w-full max-w-6xl p-4 sm:p-6">
          <Outlet />
        </div>
      </main>
      <LimbQuarantineToaster />
    </div>
  );
}
