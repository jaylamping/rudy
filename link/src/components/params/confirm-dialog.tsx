// Generic typed-confirm dialog.
//
// Supersedes the inline `ConfirmDialog` in `_authed.params.tsx`. Used for
// every destructive operation in the operator console: param writes, save
// to flash, set-zero, enable, travel-limit changes, verified-flag toggles,
// and (via `phrase`) anything else where requiring the operator to type a
// short phrase before continuing is the right friction.

import { useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export interface ConfirmDialogProps {
  title: string;
  description: ReactNode;
  /** Phrase the operator must type before the confirm button enables. */
  phrase: string;
  confirmLabel?: string;
  confirmVariant?: "default" | "destructive";
  onCancel: () => void;
  onConfirm: () => void;
}

export function ConfirmDialog({
  title,
  description,
  phrase,
  confirmLabel = "Confirm",
  confirmVariant = "destructive",
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  const [typed, setTyped] = useState("");
  return (
    <Dialog open onOpenChange={(o) => !o && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label htmlFor="confirm-phrase" className="text-sm">
            Type <code className="font-mono">{phrase}</code> to confirm:
          </Label>
          <Input
            id="confirm-phrase"
            className="font-mono"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            autoFocus
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            variant={confirmVariant}
            disabled={typed !== phrase}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
