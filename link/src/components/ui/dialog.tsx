// Lightweight modal dialog. Matches the shadcn Dialog API surface that
// this codebase uses (Dialog/DialogContent/DialogHeader/Title/Description/
// DialogFooter), backed by a fixed-position overlay + ESC/click-outside
// dismissal — the same pattern the existing `ConfirmDialog` uses but
// extracted so detail-page callers don't reinvent it.
//
// We don't need the full Radix Dialog feature set (focus trap with portals,
// nested dialogs, auto-focus management) for the operator console — every
// dialog here is a simple confirm with two-or-three buttons.

import {
  createContext,
  forwardRef,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { cn } from "@/lib/utils";

interface DialogContextValue {
  open: boolean;
  setOpen: (open: boolean) => void;
}

const DialogContext = createContext<DialogContextValue | null>(null);

function useDialog(): DialogContextValue {
  const ctx = useContext(DialogContext);
  if (!ctx) throw new Error("Dialog subcomponents must be used inside <Dialog>");
  return ctx;
}

export interface DialogProps {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: ReactNode;
}

export function Dialog({
  open,
  defaultOpen,
  onOpenChange,
  children,
}: DialogProps) {
  const [uncontrolled, setUncontrolled] = useState(defaultOpen ?? false);
  const isControlled = open !== undefined;
  const current = isControlled ? open : uncontrolled;
  const setOpen = (next: boolean) => {
    if (!isControlled) setUncontrolled(next);
    onOpenChange?.(next);
  };

  const ctx = { open: current, setOpen };
  return <DialogContext.Provider value={ctx}>{children}</DialogContext.Provider>;
}

export interface DialogTriggerProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** When true, render only the children (caller-owned button). */
  asChild?: boolean;
}

export const DialogTrigger = forwardRef<HTMLButtonElement, DialogTriggerProps>(
  ({ children, asChild, onClick, ...props }, ref) => {
    const ctx = useDialog();
    if (asChild) {
      return <>{children}</>;
    }
    return (
      <button
        ref={ref}
        type="button"
        onClick={(e) => {
          onClick?.(e);
          if (!e.defaultPrevented) ctx.setOpen(true);
        }}
        {...props}
      >
        {children}
      </button>
    );
  },
);
DialogTrigger.displayName = "DialogTrigger";

export interface DialogContentProps
  extends React.HTMLAttributes<HTMLDivElement> {
  onEscapeKeyDown?: (event: KeyboardEvent) => void;
}

export const DialogContent = forwardRef<HTMLDivElement, DialogContentProps>(
  ({ className, children, onEscapeKeyDown, ...props }, ref) => {
    const ctx = useDialog();

    useEffect(() => {
      if (!ctx.open) return;
      const onKey = (e: KeyboardEvent) => {
        if (e.key === "Escape") {
          onEscapeKeyDown?.(e);
          if (!e.defaultPrevented) ctx.setOpen(false);
        }
      };
      window.addEventListener("keydown", onKey);
      return () => window.removeEventListener("keydown", onKey);
    }, [ctx, onEscapeKeyDown]);

    if (!ctx.open) return null;

    // Portals avoid `position: fixed` being trapped by scroll/sticky
    // ancestors with `transform` / `backdrop-filter` (e.g. the app shell
    // header’s `backdrop-blur`), which would pin the overlay to the top.
    if (typeof document === "undefined") return null;

    return createPortal(
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
        onClick={(e) => {
          if (e.target === e.currentTarget) ctx.setOpen(false);
        }}
      >
        <div
          ref={ref}
          role="dialog"
          aria-modal="true"
          className={cn(
            "w-full max-w-lg space-y-4 rounded-lg border border-border bg-card p-6 shadow-lg",
            className,
          )}
          {...props}
        >
          {children}
        </div>
      </div>,
      document.body,
    );
  },
);
DialogContent.displayName = "DialogContent";

export function DialogHeader({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("flex flex-col space-y-1.5 text-left", className)}
      {...props}
    />
  );
}
DialogHeader.displayName = "DialogHeader";

export function DialogFooter({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end",
        className,
      )}
      {...props}
    />
  );
}
DialogFooter.displayName = "DialogFooter";

export const DialogTitle = forwardRef<
  HTMLHeadingElement,
  React.HTMLAttributes<HTMLHeadingElement>
>(({ className, ...props }, ref) => (
  <h3
    ref={ref}
    className={cn("text-lg font-semibold leading-none tracking-tight", className)}
    {...props}
  />
));
DialogTitle.displayName = "DialogTitle";

export const DialogDescription = forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
));
DialogDescription.displayName = "DialogDescription";
