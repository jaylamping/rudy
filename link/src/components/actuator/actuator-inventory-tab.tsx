// Inventory tab: surfaces the per-motor commissioning record (every field
// in `inventory.yaml` for this motor, including the free-form `extra` map
// the typed loader passes through).
//
// Also hosts the "mark verified" toggle that flips the `verified` flag in
// `inventory.yaml` and audits the change.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { queryKeys } from "@/api";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/params";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { OfflineActionTooltip } from "./offline-action-tooltip";
import type { JointKind } from "@/lib/types/JointKind";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import { useDeviceLive, useDeviceOfflineTip } from "@/store";

// SCAFFOLD: temporary inventory-tab-based limb assignment.
// Future UX: drag-and-drop kinematic-tree assignment in a dedicated
// "Robot setup" view. For now this gets the data into inventory.yaml so
// `POST /api/home_all` and limb-aware UI grouping can land.
const LIMB_OPTIONS = [
  "left_arm",
  "right_arm",
  "left_leg",
  "right_leg",
  "torso",
  "head",
] as const;

const LIMB_JOINTS: Record<string, JointKind[]> = {
  left_arm: [
    "shoulder_pitch",
    "shoulder_roll",
    "upper_arm_yaw",
    "elbow_pitch",
    "forearm_roll",
    "wrist_pitch",
    "wrist_yaw",
    "wrist_roll",
    "gripper",
  ],
  right_arm: [
    "shoulder_pitch",
    "shoulder_roll",
    "upper_arm_yaw",
    "elbow_pitch",
    "forearm_roll",
    "wrist_pitch",
    "wrist_yaw",
    "wrist_roll",
    "gripper",
  ],
  left_leg: [
    "hip_yaw",
    "hip_roll",
    "hip_pitch",
    "knee_pitch",
    "ankle_pitch",
    "ankle_roll",
  ],
  right_leg: [
    "hip_yaw",
    "hip_roll",
    "hip_pitch",
    "knee_pitch",
    "ankle_pitch",
    "ankle_roll",
  ],
  torso: ["waist_rotation", "spine_pitch"],
  head: ["neck_pitch", "neck_yaw"],
};

export function ActuatorInventoryTab({ motor }: { motor: MotorSummary }) {
  const isLive = useDeviceLive(motor.role);
  const offlineTip = useDeviceOfflineTip(motor.role);
  const qc = useQueryClient();
  const detail = useQuery({
    queryKey: queryKeys.inventory.byRole(motor.role),
    queryFn: () => api.getInventory(motor.role),
    retry: false,
  });
  const supported = !(
    detail.isError && (detail.error as ApiError | undefined)?.status === 404
  );

  const [confirm, setConfirm] = useState(false);
  const [note, setNote] = useState("");
  const verified = motor.verified;

  const setVerified = useMutation({
    mutationFn: () => api.setVerified(motor.role, { verified: !verified, note }),
    onSuccess: () => {
      setConfirm(false);
      setNote("");
      qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
      qc.invalidateQueries({ queryKey: queryKeys.inventory.byRole(motor.role) });
    },
  });

  return (
    <div className="space-y-4">
      <LimbAssignmentCard
        motor={motor}
        isLive={isLive}
        offlineTip={offlineTip}
      />
      <Card>
        <CardHeader className="flex flex-row items-baseline justify-between space-y-0">
          <div className="space-y-1">
            <CardTitle className="text-base">Verified status</CardTitle>
            <CardDescription>
              Verified motors are eligible for enable / jog / tests. Flip to
              unverified to lock down a motor under maintenance.
            </CardDescription>
          </div>
          {!isLive ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="inline-flex">
                  <Switch
                    checked={verified}
                    disabled
                    onCheckedChange={() => setConfirm(true)}
                    aria-label="Verified (offline)"
                  />
                </span>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs whitespace-normal" side="left">
                {offlineTip}
              </TooltipContent>
            </Tooltip>
          ) : (
            <Switch
              checked={verified}
              disabled={!supported || setVerified.isPending}
              onCheckedChange={() => setConfirm(true)}
            />
          )}
        </CardHeader>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Commissioning record</CardTitle>
          <CardDescription>
            Read-only view of <code className="font-mono">inventory.yaml</code>{" "}
            for this motor. Includes commissioning notes, baseline parameter
            dumps, and per-field timestamps.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {!supported && (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-400">
              Inventory passthrough endpoint is not yet deployed on this
              cortex build.
            </p>
          )}
          {detail.isPending && supported && (
            <div className="text-xs text-muted-foreground">Loading...</div>
          )}
          {detail.data && (
            <pre className="overflow-x-auto rounded-md border border-border bg-background p-3 text-xs">
              {JSON.stringify(detail.data, null, 2)}
            </pre>
          )}
        </CardContent>
      </Card>

      {confirm && (
        <ConfirmDialog
          title={verified ? "Mark unverified" : "Mark verified"}
          description={
            <div className="space-y-3">
              <p>
                Set <code className="font-mono">{motor.role}</code> to{" "}
                <code className="font-mono">
                  verified = {String(!verified)}
                </code>
                . The change is written to{" "}
                <code className="font-mono">inventory.yaml</code> and audit-logged.
                {!verified &&
                  " Verified motors become eligible for enable / jog / tests."}
                {verified &&
                  " Unverified motors cannot be enabled or jogged until re-verified."}
              </p>
              <Label className="space-y-1 text-sm">
                <span>Operator note (optional)</span>
                <Input
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  placeholder="e.g. recommissioned 2026-04-18"
                />
              </Label>
            </div>
          }
          confirmLabel={verified ? "Unverify" : "Verify"}
          confirmVariant={verified ? "destructive" : "default"}
          onCancel={() => {
            setConfirm(false);
            setNote("");
          }}
          onConfirm={() => setVerified.mutate()}
        />
      )}

      {setVerified.isError && (
        <p className="text-xs text-destructive">
          {(setVerified.error as ApiError).message}
        </p>
      )}
    </div>
  );
}

// limb + joint_kind + `role` must stay the canonical `limb.joint_kind`.
// Always call `/api/motors/:role/assign` when both are set: it rewrites
// `role`, `limb`, and `joint_kind` together. Using `/rename` here was wrong
// for already-assigned motors because rename only changes `role`, which
// breaks inventory validation when the operator changes limb or joint.
function LimbAssignmentCard({
  motor,
  isLive,
  offlineTip,
}: {
  motor: MotorSummary;
  isLive: boolean;
  offlineTip: string;
}) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [limb, setLimb] = useState<string>(motor.limb ?? "");
  const [joint, setJoint] = useState<JointKind | "">(motor.joint_kind ?? "");
  const previewRole =
    limb && joint ? `${limb}.${joint}` : motor.role;
  const isAssigned = !!(motor.limb && motor.joint_kind);
  const canonicalFromSummary =
    motor.limb && motor.joint_kind
      ? `${motor.limb}.${motor.joint_kind}`
      : null;
  const roleOutOfSync =
    canonicalFromSummary !== null && canonicalFromSummary !== motor.role;

  // We hold the auto-stop / auto-reenable flags from the last successful
  // rename so the operator gets one notice on the new-role page after the
  // navigate lands. Cleared the next time they touch the form.
  const [lastAutoFlags, setLastAutoFlags] = useState<{
    auto_stopped: boolean;
    auto_reenabled: boolean;
    auto_reenable_error?: string;
  } | null>(null);

  const assign = useMutation({
    mutationFn: async () => {
      if (!limb || !joint) throw new Error("limb and joint_kind required");
      return await api.assignMotor(motor.role, { limb, joint_kind: joint });
    },
    onSuccess: async (resp) => {
      // The role just changed under us. Before we navigate, the motors
      // cache must reflect the new role — otherwise the detail route's loader
      // calls `ensureQueryData({ queryKey: queryKeys.motors.all() })`, gets
      // the stale cached array, can't find `resp.new_role`, and renders
      // NotFoundActuator until you hit refresh. So we explicitly refetch
      // and *await* it.
      //
      // Also: drop the old inventory cache entry, since the old key
      // (`queryKeys.inventory.byRole(motor.role)`) now points at a role the
      // daemon no longer knows about.
      qc.removeQueries({ queryKey: queryKeys.inventory.byRole(motor.role), exact: true });
      await qc.refetchQueries({ queryKey: queryKeys.motors.all(), exact: true });
      if (resp.auto_stopped) {
        setLastAutoFlags({
          auto_stopped: true,
          auto_reenabled: !!resp.auto_reenabled,
          auto_reenable_error: resp.auto_reenable_error,
        });
      } else {
        setLastAutoFlags(null);
      }
      navigate({
        to: "/actuators/$role",
        params: { role: resp.new_role },
        search: { tab: "inventory" },
        replace: true,
      });
    },
  });

  const allowedJoints = limb && LIMB_JOINTS[limb] ? LIMB_JOINTS[limb] : [];

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Limb assignment</CardTitle>
        <CardDescription>
          {isAssigned
            ? "Update limb / joint to move this actuator on the tree. The server keeps role, limb, and joint_kind in one atomic write and emits MotorRenamed when the role string changes."
            : "Pick a limb and joint kind to enable homing under POST /api/home_all. The daemon derives the canonical role."}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {roleOutOfSync && (
          <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-amber-800 dark:text-amber-200">
            <span className="font-medium">Inventory mismatch: </span>
            <code className="font-mono">role</code> is{" "}
            <code className="font-mono">{motor.role}</code> but limb and joint
            kind imply <code className="font-mono">{canonicalFromSummary}</code>
            . Choose the correct limb and joint below, then click{" "}
            {isAssigned ? "Update assignment" : "Assign"} to repair.
          </p>
        )}
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-1.5">
            <Label className="text-sm">Limb</Label>
            <select
              className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
              value={limb}
              onChange={(e) => {
                setLimb(e.target.value);
                setJoint("");
              }}
              disabled={assign.isPending || !isLive}
            >
              <option value="">(unassigned)</option>
              {LIMB_OPTIONS.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label className="text-sm">Joint kind</Label>
            <select
              className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
              value={joint}
              onChange={(e) => setJoint(e.target.value as JointKind | "")}
              disabled={!limb || assign.isPending || !isLive}
            >
              <option value="">(unassigned)</option>
              {allowedJoints.map((j) => (
                <option key={j} value={j}>
                  {j}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="rounded-md border border-border bg-background p-2 text-xs">
          <span className="text-muted-foreground">new role: </span>
          <code className="font-mono">{previewRole}</code>
        </div>
        <div className="flex justify-end">
          <OfflineActionTooltip isLive={isLive} offlineTip={offlineTip}>
            <Button
              disabled={
                !limb ||
                !joint ||
                assign.isPending ||
                previewRole === motor.role ||
                !isLive
              }
              onClick={() => assign.mutate()}
            >
              {assign.isPending
                ? "Saving..."
                : isAssigned
                  ? "Update assignment"
                  : "Assign"}
            </Button>
          </OfflineActionTooltip>
        </div>
        {assign.isError && (() => {
          const e = assign.error as ApiError;
          const detail =
            e.body && typeof e.body === "object" && "detail" in e.body
              ? String((e.body as { detail?: unknown }).detail ?? "")
              : "";
          return (
            <p className="text-xs text-destructive">
              {detail || e.message}
            </p>
          );
        })()}
        {/* Post-rename notice: the daemon transparently dropped torque on
            the bus before performing the rename. If it managed to restore
            the enable state under the new role we say so; if not, point
            the operator at the Controls tab to re-enable manually. */}
        {lastAutoFlags?.auto_stopped && lastAutoFlags.auto_reenabled && (
          <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-amber-400">
            Motor was active during the rename. Torque was briefly dropped and
            the motor was re-enabled under the new role.
          </p>
        )}
        {lastAutoFlags?.auto_stopped && !lastAutoFlags.auto_reenabled && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
            Motor was active during the rename. Auto-stop succeeded but
            re-enable failed
            {lastAutoFlags.auto_reenable_error
              ? `: ${lastAutoFlags.auto_reenable_error}`
              : ""}
            . Use the Controls tab to re-enable when ready.
          </p>
        )}
      </CardContent>
    </Card>
  );
}
