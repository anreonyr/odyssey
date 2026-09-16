// Discriminated renderer for the six `AgentStreamEvent` kinds.
// Per the locked aesthetic, this is where the brutalist identity
// lives — ASCII kind prefixes, `⟦ ⟧` brackets around tool calls,
// mono everywhere, sharp corners on every panel.
//
// The renderer collapses consecutive `llm_delta` + `llm_reply_text`
// events into a single streaming buffer, so the user sees one
// assistant bubble that grows token-by-token with a blinking caret
// rather than a flood of single-character rows. The grouping is
// purely visual — the underlying `events` array is untouched so
// downstream consumers can replay the raw stream.

import type { AgentStreamEvent } from "../api/types";

import { cn } from "../lib/utils";

// ---------------- Grouping ----------------

type RenderRow =
  | { kind: "user"; text: string; firstAt: number }
  | { kind: "llm"; text: string; firstAt: number; lastAt: number; live: boolean }
  | { kind: "tool_call"; event: Extract<AgentStreamEvent, { kind: "tool_call" }>; at: number }
  | { kind: "tool_result"; event: Extract<AgentStreamEvent, { kind: "tool_result" }>; at: number }
  | { kind: "final"; event: Extract<AgentStreamEvent, { kind: "final" }>; at: number }
  | { kind: "error"; event: Extract<AgentStreamEvent, { kind: "error" }>; at: number };

export function groupEvents(events: AgentStreamEvent[], live: boolean): RenderRow[] {
  const rows: RenderRow[] = [];
  for (const evt of events) {
    const last = rows[rows.length - 1];
    if (evt.kind === "llm_delta" || evt.kind === "llm_reply_text") {
      const text = evt.kind === "llm_delta" ? evt.text : evt.text;
      if (last?.kind === "llm") {
        last.text += text;
        last.lastAt = Date.now();
      } else {
        rows.push({ kind: "llm", text, firstAt: Date.now(), lastAt: Date.now(), live: false });
      }
    } else if (evt.kind === "tool_call") {
      rows.push({ kind: "tool_call", event: evt, at: Date.now() });
    } else if (evt.kind === "tool_result") {
      rows.push({ kind: "tool_result", event: evt, at: Date.now() });
    } else if (evt.kind === "final") {
      rows.push({ kind: "final", event: evt, at: Date.now() });
    } else if (evt.kind === "error") {
      rows.push({ kind: "error", event: evt, at: Date.now() });
    }
  }
  // Mark the trailing llm row as live so the caret blinks while
  // more deltas are in flight.
  const tail = rows[rows.length - 1];
  if (tail?.kind === "llm" && live) tail.live = true;
  return rows;
}

// ---------------- Timestamp helper ----------------

function fmtTime(ts: number): string {
  const d = new Date(ts);
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  return `${h}:${m}:${s}`;
}

// ---------------- Row components ----------------

export function StreamEventRow({ row, step }: { row: RenderRow; step: number }) {
  if (row.kind === "llm") return <LlmRow row={row} step={step} />;
  if (row.kind === "tool_call") return <ToolCallRow row={row} step={step} />;
  if (row.kind === "tool_result") return <ToolResultRow row={row} step={step} />;
  if (row.kind === "final") return <FinalRow row={row} step={step} />;
  if (row.kind === "error") return <ErrorRow row={row} step={step} />;
  return null;
}

function Rail({
  prefix,
  time,
  step,
  tone,
}: {
  prefix: string;
  time: string;
  step: number;
  tone: "muted" | "llm" | "tool" | "result" | "done" | "error";
}) {
  const colorClass = {
    muted: "text-muted-foreground",
    llm: "text-primary",
    tool: "text-warning",
    result: "text-muted-foreground",
    done: "text-success",
    error: "text-destructive",
  }[tone];
  return (
    <div
      className={cn(
        "border-border bg-card/40 flex w-32 shrink-0 flex-col items-start gap-1 border-r-2 p-3 font-mono text-[10px] uppercase tracking-widest",
        colorClass,
      )}
    >
      <span className="font-bold">{prefix}</span>
      <span className="text-muted-foreground normal-case tabular-nums tracking-tight">{time}</span>
      <span className="text-muted-foreground tabular-nums">step {step}</span>
    </div>
  );
}

function Panel({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn("border-border flex-1 border-l-0 p-3", className)}>{children}</div>;
}

function LlmRow({ row, step }: { row: Extract<RenderRow, { kind: "llm" }>; step: number }) {
  return (
    <div className="border-border bg-background flex border-2">
      <Rail prefix="≡ LLM" time={fmtTime(row.firstAt)} step={step} tone="llm" />
      <Panel className="bg-background">
        <pre className="text-foreground whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {row.text}
          {row.live && (
            <span className="text-primary ml-0.5 inline-block animate-pulse font-bold">▌</span>
          )}
        </pre>
      </Panel>
    </div>
  );
}

function ToolCallRow({
  row,
  step,
}: {
  row: Extract<RenderRow, { kind: "tool_call" }>;
  step: number;
}) {
  return (
    <div className="border-warning/40 bg-warning/5 flex border-2">
      <Rail prefix={`◇ TOOL · ${row.event.tool}`} time={fmtTime(row.at)} step={step} tone="tool" />
      <Panel>
        <pre className="text-foreground font-mono text-[11px] leading-relaxed">
          <span className="text-warning">⟦</span> {row.event.tool}.invoke(
          {JSON.stringify(row.event.args, null, 2)}) <span className="text-warning">⟧</span>
        </pre>
      </Panel>
    </div>
  );
}

function ToolResultRow({
  row,
  step,
}: {
  row: Extract<RenderRow, { kind: "tool_result" }>;
  step: number;
}) {
  const outcome = row.event.outcome;
  return (
    <div
      className={cn(
        "flex border-2",
        outcome.ok ? "border-border bg-card" : "border-destructive/40 bg-destructive/5",
      )}
    >
      <Rail
        prefix={`→ RESULT · ${row.event.tool}`}
        time={fmtTime(row.at)}
        step={step}
        tone={outcome.ok ? "result" : "error"}
      />
      <Panel>
        {outcome.ok ? (
          <pre className="text-foreground whitespace-pre-wrap font-mono text-[11px] leading-relaxed">
            {stringifyCompact(outcome.value)}
          </pre>
        ) : (
          <pre className="text-destructive font-mono text-[11px] leading-relaxed">
            × {outcome.error}
          </pre>
        )}
      </Panel>
    </div>
  );
}

function FinalRow({ row, step }: { row: Extract<RenderRow, { kind: "final" }>; step: number }) {
  return (
    <div className="border-success/40 bg-success/5 flex border-2">
      <Rail
        prefix={`■ DONE · ${row.event.reason}`}
        time={fmtTime(row.at)}
        step={step}
        tone="done"
      />
      <Panel>
        <pre className="text-foreground whitespace-pre-wrap font-mono text-[11px] leading-relaxed">
          {stringifyCompact(row.event.value)}
        </pre>
      </Panel>
    </div>
  );
}

function ErrorRow({ row, step }: { row: Extract<RenderRow, { kind: "error" }>; step: number }) {
  return (
    <div className="border-destructive/50 bg-destructive/10 flex border-2">
      <Rail prefix="× ERROR" time={fmtTime(row.at)} step={step} tone="error" />
      <Panel>
        <pre className="text-destructive whitespace-pre-wrap font-mono text-[11px] leading-relaxed">
          {row.event.message}
        </pre>
      </Panel>
    </div>
  );
}

function stringifyCompact(v: unknown): string {
  if (v == null) return "null";
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}
