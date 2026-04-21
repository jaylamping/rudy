// Tabs primitive with the shadcn API surface. State lives in a TabsContext
// (uncontrolled by default; pass `value`+`onValueChange` to control).
//
// Why hand-rolled instead of @radix-ui/react-tabs:
//   * `radix-ui` (the meta-package the SPA already pulls) bundles the same
//     primitives but doesn't ship explicit TS types per primitive — and we
//     only need the tiny tab-button + content-pane behavior here. Hand-roll
//     to keep dependency graph + bundle size small.
//
// Keyboard: ArrowLeft / ArrowRight cycle through TabsTrigger siblings.

import {
  createContext,
  forwardRef,
  useContext,
  useId,
  useRef,
  useState,
} from "react";
import { cn } from "@/lib/utils";

interface TabsContextValue {
  value: string;
  setValue: (v: string) => void;
  baseId: string;
}

const TabsContext = createContext<TabsContextValue | null>(null);

function useTabs(): TabsContextValue {
  const ctx = useContext(TabsContext);
  if (!ctx) throw new Error("Tabs subcomponents must be used inside <Tabs>");
  return ctx;
}

export interface TabsProps extends React.HTMLAttributes<HTMLDivElement> {
  defaultValue?: string;
  value?: string;
  onValueChange?: (value: string) => void;
}

export function Tabs({
  defaultValue,
  value,
  onValueChange,
  children,
  className,
  ...props
}: TabsProps) {
  const baseId = useId();
  const [uncontrolled, setUncontrolled] = useState(defaultValue ?? "");
  const isControlled = value !== undefined;
  const current = isControlled ? value : uncontrolled;
  const setValue = (v: string) => {
    if (!isControlled) setUncontrolled(v);
    onValueChange?.(v);
  };

  const ctx = { value: current, setValue, baseId };

  return (
    <TabsContext.Provider value={ctx}>
      <div className={className} {...props}>
        {children}
      </div>
    </TabsContext.Provider>
  );
}

export const TabsList = forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    role="tablist"
    className={cn(
      "inline-flex h-9 items-center justify-center rounded-lg bg-muted p-1 text-muted-foreground",
      className,
    )}
    {...props}
  />
));
TabsList.displayName = "TabsList";

export interface TabsTriggerProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  value: string;
}

export const TabsTrigger = forwardRef<HTMLButtonElement, TabsTriggerProps>(
  ({ value, className, onKeyDown, ...props }, ref) => {
    const ctx = useTabs();
    const active = ctx.value === value;
    const localRef = useRef<HTMLButtonElement | null>(null);

    const handleKey = (e: React.KeyboardEvent<HTMLButtonElement>) => {
      onKeyDown?.(e);
      if (e.defaultPrevented) return;
      if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
      const list = localRef.current?.parentElement;
      if (!list) return;
      const triggers = Array.from(
        list.querySelectorAll<HTMLButtonElement>('[role="tab"]:not([disabled])'),
      );
      const idx = triggers.indexOf(localRef.current!);
      if (idx === -1) return;
      const delta = e.key === "ArrowRight" ? 1 : -1;
      const next = triggers[(idx + delta + triggers.length) % triggers.length];
      next.focus();
      const v = next.getAttribute("data-value");
      if (v) ctx.setValue(v);
      e.preventDefault();
    };

    return (
      <button
        ref={(el) => {
          localRef.current = el;
          if (typeof ref === "function") ref(el);
          else if (ref) ref.current = el;
        }}
        role="tab"
        type="button"
        aria-selected={active}
        aria-controls={`${ctx.baseId}-${value}-panel`}
        id={`${ctx.baseId}-${value}-tab`}
        tabIndex={active ? 0 : -1}
        data-state={active ? "active" : "inactive"}
        data-value={value}
        onClick={() => ctx.setValue(value)}
        onKeyDown={handleKey}
        className={cn(
          "inline-flex items-center justify-center whitespace-nowrap rounded-md px-3 py-1 text-sm font-medium ring-offset-background transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
          active
            ? "bg-background text-foreground shadow"
            : "text-muted-foreground hover:text-foreground",
          className,
        )}
        {...props}
      />
    );
  },
);
TabsTrigger.displayName = "TabsTrigger";

export interface TabsContentProps extends React.HTMLAttributes<HTMLDivElement> {
  value: string;
}

export const TabsContent = forwardRef<HTMLDivElement, TabsContentProps>(
  ({ value, className, ...props }, ref) => {
    const ctx = useTabs();
    const active = ctx.value === value;
    if (!active) return null;
    return (
      <div
        ref={ref}
        role="tabpanel"
        id={`${ctx.baseId}-${value}-panel`}
        aria-labelledby={`${ctx.baseId}-${value}-tab`}
        tabIndex={0}
        className={cn(
          "mt-2 ring-offset-background focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          className,
        )}
        {...props}
      />
    );
  },
);
TabsContent.displayName = "TabsContent";
