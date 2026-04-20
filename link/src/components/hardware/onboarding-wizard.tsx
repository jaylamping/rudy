/**
 * Onboard an unassigned CAN device as a RobStride actuator (inventory append).
 * Commission + verify are separate steps using existing REST endpoints.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Loader2 } from "lucide-react";
import { ApiError, api } from "@/lib/api";
import { Button, buttonVariants } from "@/components/ui/button";
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
import type { JointKind } from "@/lib/types/JointKind";
import type { UnassignedDevice } from "@/lib/types/UnassignedDevice";
import { cn } from "@/lib/utils";

const JOINT_KINDS: JointKind[] = [
  "waist_rotation",
  "spine_pitch",
  "shoulder_pitch",
  "shoulder_roll",
  "upper_arm_yaw",
  "elbow_pitch",
  "forearm_roll",
  "wrist_pitch",
  "wrist_yaw",
  "wrist_roll",
  "gripper",
  "hip_yaw",
  "hip_roll",
  "hip_pitch",
  "knee_pitch",
  "ankle_pitch",
  "ankle_roll",
  "neck_pitch",
  "neck_yaw",
];

/** ±30° in rad — conservative default band inside typical RS03 rail. */
const DEFAULT_HALF_SPAN_RAD = (30 * Math.PI) / 180;

function specLabelToModelId(label: string): string {
  return label.trim().toLowerCase();
}

function formatErr(e: unknown): string {
  if (e instanceof ApiError) {
    const d =
      e.body && typeof e.body === "object" && "detail" in e.body
        ? String((e.body as { detail?: unknown }).detail ?? "")
        : "";
    return d ? `${e.message}: ${d}` : e.message;
  }
  if (e instanceof Error) return e.message;
  return String(e);
}

export function OnboardingWizard({
  open,
  onOpenChange,
  device,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  device: UnassignedDevice | null;
}) {
  const qc = useQueryClient();
  const configQ = useQuery({ queryKey: ["config"], queryFn: () => api.config() });

  const modelOptions = useMemo(() => {
    const labels = configQ.data?.actuator_models ?? ["RS03"];
    return labels.map(specLabelToModelId);
  }, [configQ.data?.actuator_models]);

  const [model, setModel] = useState<string>("rs03");
  const [limb, setLimb] = useState("left_arm");
  const [jointKind, setJointKind] = useState<JointKind>("shoulder_pitch");
  const [travelMin, setTravelMin] = useState(String(-DEFAULT_HALF_SPAN_RAD));
  const [travelMax, setTravelMax] = useState(String(DEFAULT_HALF_SPAN_RAD));
  const [homeRad, setHomeRad] = useState("0");
  const [phase, setPhase] = useState<"form" | "after">("form");
  const [createdRole, setCreatedRole] = useState<string | null>(null);

  useEffect(() => {
    if (open && device) {
      setPhase("form");
      setCreatedRole(null);
      onboardMut.reset();
      commissionMut.reset();
      verifyMut.reset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset when target device changes
  }, [open, device?.bus, device?.can_id]);

  const derivedRole = `${limb}.${jointKind}`;

  const onboardMut = useMutation({
    mutationFn: () => {
      if (!device) throw new Error("no device");
      return api.onboardRobstrideActuator({
        can_bus: device.bus,
        can_id: device.can_id,
        model: model as "rs01" | "rs02" | "rs03" | "rs04",
        limb,
        joint_kind: jointKind,
        travel_min_rad: Number(travelMin),
        travel_max_rad: Number(travelMax),
        predefined_home_rad: homeRad === "" ? undefined : Number(homeRad),
      });
    },
    onSuccess: (data) => {
      setCreatedRole(data.role);
      setPhase("after");
      void qc.invalidateQueries({ queryKey: ["devices"] });
      void qc.invalidateQueries({ queryKey: ["motors"] });
      void qc.invalidateQueries({ queryKey: ["hardware", "unassigned"] });
    },
  });

  const commissionMut = useMutation({
    mutationFn: () => {
      if (!createdRole) throw new Error("no role");
      return api.commissionMotor(createdRole);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["motors"] });
      void qc.invalidateQueries({ queryKey: ["devices"] });
    },
  });

  const verifyMut = useMutation({
    mutationFn: () => {
      if (!createdRole) throw new Error("no role");
      return api.setVerified(createdRole, { verified: true });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });

  const resetAndClose = () => {
    setPhase("form");
    setCreatedRole(null);
    onboardMut.reset();
    commissionMut.reset();
    verifyMut.reset();
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Onboard device</DialogTitle>
          <DialogDescription>
            Add this CAN node to <code className="text-xs">inventory.yaml</code> as a RobStride
            actuator. Position the joint, then commission flash zero when ready.
          </DialogDescription>
        </DialogHeader>

        {!device ? null : phase === "form" ? (
          <div className="space-y-4">
            <div className="rounded-md border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
              <div>
                Bus <span className="font-mono text-foreground">{device.bus}</span> · CAN ID{" "}
                <span className="font-mono text-foreground">
                  0x{device.can_id.toString(16).padStart(2, "0")}
                </span>
              </div>
              {device.family_hint ? (
                <div>
                  Scan hint: <span className="text-foreground">{device.family_hint}</span>
                </div>
              ) : null}
            </div>

            {device.family_hint && device.family_hint !== "robstride" ? (
              <p className="text-sm text-amber-700 dark:text-amber-400">
                Family hint is not <code className="text-xs">robstride</code> — this flow only
                configures RobStride actuators. Adjust fields carefully.
              </p>
            ) : null}

            <div className="space-y-2">
              <Label htmlFor="onb-model">RobStride model</Label>
              <select
                id="onb-model"
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              >
                {modelOptions.map((m) => (
                  <option key={m} value={m}>
                    {m.toUpperCase()}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="onb-limb">Limb</Label>
              <Input
                id="onb-limb"
                value={limb}
                onChange={(e) => setLimb(e.target.value)}
                placeholder="left_arm"
                className="font-mono text-sm"
              />
              <p className="text-xs text-muted-foreground">
                Lowercase identifier (letters, digits, underscores). Role will be{" "}
                <code className="text-xs">{derivedRole}</code>.
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="onb-joint">Joint kind</Label>
              <select
                id="onb-joint"
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
                value={jointKind}
                onChange={(e) => setJointKind(e.target.value as JointKind)}
              >
                {JOINT_KINDS.map((jk) => (
                  <option key={jk} value={jk}>
                    {jk}
                  </option>
                ))}
              </select>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label htmlFor="onb-min">Travel min (rad)</Label>
                <Input
                  id="onb-min"
                  type="number"
                  step="0.01"
                  value={travelMin}
                  onChange={(e) => setTravelMin(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="onb-max">Travel max (rad)</Label>
                <Input
                  id="onb-max"
                  type="number"
                  step="0.01"
                  value={travelMax}
                  onChange={(e) => setTravelMax(e.target.value)}
                />
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="onb-home">Predefined home (rad)</Label>
              <Input
                id="onb-home"
                type="number"
                step="0.01"
                value={homeRad}
                onChange={(e) => setHomeRad(e.target.value)}
              />
            </div>

            {onboardMut.isError ? (
              <p className="text-sm text-destructive">{formatErr(onboardMut.error)}</p>
            ) : null}
          </div>
        ) : (
          <div className="space-y-4">
            <p className="text-sm">
              Added actuator <span className="font-medium">{createdRole}</span>. Physically position
              the joint, then flash the zero to firmware.
            </p>
            {commissionMut.isError ? (
              <p className="text-sm text-destructive">{formatErr(commissionMut.error)}</p>
            ) : null}
            {verifyMut.isError ? (
              <p className="text-sm text-destructive">{formatErr(verifyMut.error)}</p>
            ) : null}
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                size="sm"
                disabled={commissionMut.isPending || commissionMut.isSuccess}
                onClick={() => commissionMut.mutate()}
              >
                {commissionMut.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : null}
                Commission zero (flash)
              </Button>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                disabled={verifyMut.isPending || verifyMut.isSuccess}
                onClick={() => verifyMut.mutate()}
              >
                Mark verified
              </Button>
            </div>
            {commissionMut.isSuccess ? (
              <p className="text-xs text-muted-foreground">Commission completed.</p>
            ) : null}
            {verifyMut.isSuccess ? (
              <p className="text-xs text-muted-foreground">Verified flag saved.</p>
            ) : null}
            {createdRole ? (
              <Link
                to="/actuators/$role"
                params={{ role: createdRole }}
                className={cn(buttonVariants({ variant: "outline", size: "sm" }))}
              >
                Open actuator page
              </Link>
            ) : null}
          </div>
        )}

        <DialogFooter className="gap-2 sm:gap-0">
          {phase === "form" ? (
            <>
              <Button type="button" variant="ghost" onClick={() => resetAndClose()}>
                Cancel
              </Button>
              <Button
                type="button"
                disabled={onboardMut.isPending}
                onClick={() => onboardMut.mutate()}
              >
                {onboardMut.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : null}
                Save to inventory
              </Button>
            </>
          ) : (
            <Button type="button" onClick={() => resetAndClose()}>
              Done
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
