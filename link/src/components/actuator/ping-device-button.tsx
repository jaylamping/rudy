import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Radio } from "lucide-react";
import { useState } from "react";
import { queryKeys } from "@/api";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";

type Props = {
  role: string;
  className?: string;
};

/** CAN presence probe; invalidates motors list on success so live state can update. */
export function PingDeviceButton({ role, className }: Props) {
  const qc = useQueryClient();
  const [lastMsg, setLastMsg] = useState<string | null>(null);

  const ping = useMutation({
    mutationFn: () => api.pingMotor(role),
    onSuccess: async (data) => {
      if (data.ok) {
        const fw = data.firmware_version
          ? ` — fw ${data.firmware_version}`
          : "";
        setLastMsg(`Answered in ${data.elapsed_ms} ms${fw}`);
        await qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
      } else {
        setLastMsg(`No response (${data.elapsed_ms} ms)`);
      }
    },
    onError: (e) => {
      const msg = e instanceof ApiError ? e.message : String(e);
      setLastMsg(msg);
    },
  });

  return (
    <div className={className}>
      <Button
        type="button"
        variant="secondary"
        size="sm"
        className="h-7 gap-1.5 whitespace-nowrap text-xs"
        disabled={ping.isPending}
        onClick={() => {
          setLastMsg(null);
          ping.mutate();
        }}
      >
        <Radio className="size-3.5" strokeWidth={2.25} />
        {ping.isPending ? "Pinging…" : "Ping device"}
      </Button>
      {lastMsg != null && (
        <p className="mt-1 max-w-[20rem] text-right text-xs text-muted-foreground">
          {lastMsg}
        </p>
      )}
    </div>
  );
}
