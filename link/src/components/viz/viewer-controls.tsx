// Floating control strip for the URDF viewer. Pure presentational; the
// parent owns state.

import { Box, Grid3x3, RotateCcw } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ViewerControlsState {
  showGrid: boolean;
  wireframe: boolean;
}

export interface ViewerControlsProps {
  state: ViewerControlsState;
  onChange: (next: ViewerControlsState) => void;
  onReset: () => void;
  className?: string;
}

export function ViewerControls({
  state,
  onChange,
  onReset,
  className,
}: ViewerControlsProps) {
  return (
    <div
      className={cn(
        "pointer-events-auto flex items-center gap-1 rounded-md border border-border bg-card/90 p-1 text-xs shadow backdrop-blur",
        className,
      )}
    >
      <ToggleButton
        active={state.showGrid}
        onClick={() => onChange({ ...state, showGrid: !state.showGrid })}
        title="Toggle grid"
      >
        <Grid3x3 className="h-3.5 w-3.5" />
      </ToggleButton>
      <ToggleButton
        active={state.wireframe}
        onClick={() => onChange({ ...state, wireframe: !state.wireframe })}
        title="Toggle wireframe"
      >
        <Box className="h-3.5 w-3.5" />
      </ToggleButton>
      <button
        type="button"
        onClick={onReset}
        title="Reset camera"
        className="flex items-center gap-1 rounded px-2 py-1 hover:bg-accent/60"
      >
        <RotateCcw className="h-3.5 w-3.5" />
        <span>Reset</span>
      </button>
    </div>
  );
}

function ToggleButton({
  active,
  onClick,
  title,
  children,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "flex items-center gap-1 rounded px-2 py-1 transition",
        active
          ? "bg-accent text-accent-foreground"
          : "text-muted-foreground hover:bg-accent/60",
      )}
    >
      {children}
    </button>
  );
}
