// Bubble-style message rendering for the chat view.
// User messages → right-aligned, primary-tinted.
// Agent text → left-aligned, card-bg, with a blinking caret
// while the LLM is still streaming.
// Tool calls / results → collapsible one-line summary that
// expands on click to show the JSON args / outcome — the user
// sees the conversation, not the agent's internals.
// Final / Error events break out of the bubble layout and
// span the column for emphasis.

import type { AgentStreamEvent } from "../api/types";

import { useState } from "react";

import { cn } from "../lib/utils";

export type RenderRow =
  | { kind: "user"; text: string; firstAt: number }
  | { kind: "llm"; text: string; firstAt: number; lastAt: number; live: boolean }
  | { kind: "tool_call"; event: Extract<AgentStreamEvent, { kind: "tool_call" }>; at: number }
  | { kind: "tool_result"; event: Extract<AgentStreamEvent, { kind: "tool_result" }>; at: number }
  | { kind: "final"; event: Extract<AgentStreamEvent, { kind: "final" }>; at: number }
  | { kind: "error"; event: Extract<AgentStreamEvent, { kind: "error" }>; at: number };

function fmtTime(ts: number): string {
  const d = new Date(ts);
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  return `${h}:${m}:${s}`;
}

// ---------------- Grouping ----------------
//
// Collapses the raw `agent_stream` event sequence into the
// `RenderRow` shape the bubbles render. Consecutive llm_delta
// / llm_reply_text events fold into a single running LLM row
// (so the user sees one bubble growing token-by-token, not a
// flood of single-character rows); tool_call, tool_result,
// final, error each get their own row with the originating
// event preserved for the collapsible JSON view.

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
        rows.push({
          kind: "llm",
          text,
          firstAt: Date.now(),
          lastAt: Date.now(),
          live: false,
        });
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

export function MessageBubble({ row }: { row: RenderRow }) {
  if (row.kind === "user") return <UserBubble row={row} />;
  if (row.kind === "llm") return <AgentBubble row={row} />;
  if (row.kind === "tool_call") return <ToolCallRow row={row} />;
  if (row.kind === "tool_result") return <ToolResultRow row={row} />;
  if (row.kind === "final") return <FinalBubble row={row} />;
  if (row.kind === "error") return <ErrorBubble row={row} />;
  return null;
}

function BubbleShell({
  side,
  children,
}: {
  side: "left" | "right" | "center";
  children: React.ReactNode;
}) {
  const justify =
    side === "right" ? "justify-end" : side === "center" ? "justify-center" : "justify-start";
  return <div className={cn("flex", justify)}>{children}</div>;
}

function UserBubble({ row }: { row: Extract<RenderRow, { kind: "user" }> }) {
  return (
    <BubbleShell side="right">
      <div
        className="bg-primary/10 border-primary/30 max-w-2xl border-2 px-4 py-2"
        data-message-kind="user"
      >
        <pre className="text-foreground whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {row.text}
        </pre>
        <div className="text-muted-foreground mt-1 text-right font-mono text-[10px] tabular-nums">
          {fmtTime(row.firstAt)}
        </div>
      </div>
    </BubbleShell>
  );
}

function AgentBubble({ row }: { row: Extract<RenderRow, { kind: "llm" }> }) {
  return (
    <BubbleShell side="left">
      <div className="bg-card border-border max-w-2xl border-2 px-4 py-2" data-message-kind="agent">
        <pre className="text-foreground whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {row.text}
          {row.live && (
            <span className="text-primary ml-0.5 inline-block animate-pulse font-bold">▌</span>
          )}
        </pre>
        <div className="text-muted-foreground mt-1 font-mono text-[10px] tabular-nums">
          {fmtTime(row.firstAt)}
        </div>
      </div>
    </BubbleShell>
  );
}

function ToolCallRow({ row }: { row: Extract<RenderRow, { kind: "tool_call" }> }) {
  const [expanded, setExpanded] = useState(false);
  const argCount = Object.keys(row.event.args || {}).length;
  return (
    <BubbleShell side="left">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="border-warning/30 bg-warning/5 hover:bg-warning/10 max-w-2xl border-2 px-3 py-1.5 text-left transition-colors"
        data-message-kind="tool-call"
        data-action="toggle-tool-call"
      >
        <div className="flex items-center gap-2 font-mono text-[11px]">
          <span className="text-muted-foreground">{expanded ? "▾" : "▸"}</span>
          <span className="text-warning">called {row.event.tool}</span>
          <span className="text-muted-foreground">
            ({argCount} arg{argCount !== 1 ? "s" : ""})
          </span>
          <span className="text-muted-foreground ml-auto tabular-nums">{fmtTime(row.at)}</span>
        </div>
        {expanded && (
          <pre className="text-foreground mt-2 font-mono text-[11px] leading-relaxed">
            {JSON.stringify(row.event.args, null, 2)}
          </pre>
        )}
      </button>
    </BubbleShell>
  );
}

function ToolResultRow({ row }: { row: Extract<RenderRow, { kind: "tool_result" }> }) {
  const [expanded, setExpanded] = useState(false);
  const outcome = row.event.outcome;
  return (
    <BubbleShell side="left">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className={cn(
          "max-w-2xl border-2 px-3 py-1.5 text-left transition-colors",
          outcome.ok
            ? "border-border bg-muted/30 hover:bg-muted/50"
            : "border-destructive/40 bg-destructive/5 hover:bg-destructive/10",
        )}
        data-message-kind="tool-result"
        data-action="toggle-tool-result"
      >
        <div className="flex items-center gap-2 font-mono text-[11px]">
          <span className="text-muted-foreground">{expanded ? "▾" : "▸"}</span>
          <span className={outcome.ok ? "text-muted-foreground" : "text-destructive"}>
            result of {row.event.tool} ({outcome.ok ? "ok" : "error"})
          </span>
          <span className="text-muted-foreground ml-auto tabular-nums">{fmtTime(row.at)}</span>
        </div>
        {expanded && (
          <pre
            className={cn(
              "mt-2 whitespace-pre-wrap font-mono text-[11px] leading-relaxed",
              outcome.ok ? "text-foreground" : "text-destructive",
            )}
          >
            {outcome.ok ? JSON.stringify(outcome.value, null, 2) : outcome.error}
          </pre>
        )}
      </button>
    </BubbleShell>
  );
}

function FinalBubble({ row }: { row: Extract<RenderRow, { kind: "final" }> }) {
  return (
    <BubbleShell side="center">
      <div
        className="bg-success/10 border-success/40 w-full max-w-2xl border-2 px-4 py-3"
        data-message-kind="final"
      >
        <div className="text-success mb-2 font-mono text-[10px] font-bold uppercase tracking-widest">
          ■ final · {row.event.reason}
        </div>
        <pre className="text-foreground whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {stringifyCompact(row.event.value)}
        </pre>
        <div className="text-muted-foreground mt-2 font-mono text-[10px] tabular-nums">
          {fmtTime(row.at)}
        </div>
      </div>
    </BubbleShell>
  );
}

function ErrorBubble({ row }: { row: Extract<RenderRow, { kind: "error" }> }) {
  return (
    <BubbleShell side="center">
      <div
        className="bg-destructive/10 border-destructive/40 w-full max-w-2xl border-2 px-4 py-3"
        data-message-kind="error"
      >
        <div className="text-destructive mb-2 font-mono text-[10px] font-bold uppercase tracking-widest">
          × error
        </div>
        <pre className="text-destructive whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {row.event.message}
        </pre>
        <div className="text-muted-foreground mt-2 font-mono text-[10px] tabular-nums">
          {fmtTime(row.at)}
        </div>
      </div>
    </BubbleShell>
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
