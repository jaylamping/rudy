import { createFileRoute, redirect } from "@tanstack/react-router";
import { isAuthed } from "@/lib/auth";

export const Route = createFileRoute("/")({
  beforeLoad: () => {
    if (!isAuthed()) {
      throw redirect({ to: "/login" });
    }
    throw redirect({ to: "/telemetry" });
  },
});
