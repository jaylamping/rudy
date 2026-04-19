import type { LogEntry } from "@/lib/types/LogEntry";
import { LevelBadge } from "./level-badge";

/** Right-pane detail view. Renders every available field in a fixed
 * order (timestamp, level, source, target, span, message, fields, then
 * the audit-only fields if present). The fields blob is pretty-printed
 * JSON with monospace; the operator copies straight out of the page
 * for bug reports. */
export function LogDetail({ entry }: { entry: LogEntry | null }) {
  if (!entry) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
        Select a log entry to inspect its full payload.
      </div>
    );
  }

  const isoTime = new Date(Number(entry.t_ms)).toISOString();
  // `entry.fields` may include BigInt values (e.g. ids deserialized from
  // SuperJSON). Native JSON.stringify throws on those, so coerce to a
  // string in the replacer. Numbers >= 2^53 lose precision either way,
  // but the operator only needs a readable view here.
  const fieldsJson = JSON.stringify(
    entry.fields,
    (_k, v) => (typeof v === "bigint" ? v.toString() : v),
    2,
  );
  const isAudit = entry.source === "audit";

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-sm">
      <div className="flex items-center gap-2">
        <LevelBadge level={entry.level} />
        <span className="font-mono text-xs text-muted-foreground">{isoTime}</span>
        <span className="ml-auto rounded-sm bg-muted px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted-foreground">
          {entry.source}
        </span>
      </div>

      <Field label="message">
        <p className="font-mono text-sm whitespace-pre-wrap break-words">{entry.message}</p>
      </Field>

      <Field label="target">
        <code className="font-mono text-xs">{entry.target}</code>
      </Field>

      {entry.span && (
        <Field label="span">
          <code className="font-mono text-xs">{entry.span}</code>
        </Field>
      )}

      {fieldsJson !== "{}" && (
        <Field label="fields">
          <pre className="overflow-x-auto rounded-sm border border-border bg-background p-2 font-mono text-[11px] leading-snug">
            {fieldsJson}
          </pre>
        </Field>
      )}

      {isAudit && (
        <>
          {entry.action && (
            <Field label="audit.action">
              <code className="font-mono text-xs">{entry.action}</code>
            </Field>
          )}
          {entry.audit_target && (
            <Field label="audit.target">
              <code className="font-mono text-xs">{entry.audit_target}</code>
            </Field>
          )}
          {entry.result && (
            <Field label="audit.result">
              <code className="font-mono text-xs">{entry.result}</code>
            </Field>
          )}
          {entry.session_id && (
            <Field label="audit.session_id">
              <code className="font-mono text-xs">{entry.session_id}</code>
            </Field>
          )}
          {entry.remote && (
            <Field label="audit.remote">
              <code className="font-mono text-xs">{entry.remote}</code>
            </Field>
          )}
        </>
      )}

      <Field label="id">
        <code className="font-mono text-xs text-muted-foreground">{String(entry.id)}</code>
      </Field>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">{label}</div>
      {children}
    </div>
  );
}
