// Generic confirm dialog.
//
// Used for every destructive operation in the operator console: param
// writes, save to flash, set-zero, enable, travel-limit changes,
// verified-flag toggles, etc.
//
// This is a plain "are you sure?" modal — no typed-phrase friction.
// Rationale: Rudy is a single-operator tailnet-bounded console (see
// ADR-0004 D5), so the misclick-prevention value of forcing the operator
// to type a phrase is dominated by the day-to-day cost of typing it. The
// firmware envelope + travel-limits + dead-man jog + audit log are the
// real safety story; the modal is a "did you mean to" pause and nothing
// more.

import { type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export interface ConfirmDialogProps {
  title: string;
  description: ReactNode;
  confirmLabel?: string;
  confirmVariant?: "default" | "destructive";
  onCancel: () => void;
  onConfirm: () => void;
}

export function ConfirmDialog({
  title,
  description,
  confirmLabel = "Confirm",
  confirmVariant = "destructive",
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <Dialog open onOpenChange={(o) => !o && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            variant={confirmVariant}
            onClick={onConfirm}
            autoFocus
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
