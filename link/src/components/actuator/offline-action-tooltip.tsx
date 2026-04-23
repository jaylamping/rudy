import type { ReactElement } from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

type Props = {
  isLive: boolean;
  offlineTip: string;
  children: ReactElement;
};

/** When the device is offline, wrap a disabled control in a tooltip explaining why. */
export function OfflineActionTooltip({
  isLive,
  offlineTip,
  children,
}: Props) {
  if (isLive) return children;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex w-full sm:w-auto">{children}</span>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs whitespace-normal" side="bottom">
        {offlineTip}
      </TooltipContent>
    </Tooltip>
  );
}
