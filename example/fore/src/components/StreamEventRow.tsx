// Bubble-style message rendering for the chat view.
// User messages → right-aligned, primary-tinted.
// Agent text → left-aligned, card-bg, with a blinking caret
// while the LLM is still streaming.
// Tool calls / results → collapsible one-line summary that
// expands on click to show the JSON args / outcome — the user
// sees the conversation, not the agent's internals.
// Final / Error events break out of the bubble layout and
// span the column for emphasis.

import type { AgentStreamEvent } from "@/api/types";

import { useState } from "react";

import { Button } from "@/components/ui/button";

export type RenderRow =
  | { kind: "user"; text: string; firstAt: number }
  | { kind: "llm"; text: string; firstAt: number; lastAt: number; live: boolean }
  | { kind: "tool_call"; event: Extract<AgentStreamEvent, { kind: "tool_call" }>; at: number }
  | { kind: "tool_result"; event: Extract<AgentStreamEvent, { kind: "tool_result" }>; at: number }
  | { kind: "final"; event: Extract<AgentStreamEvent, { kind: "final" }>; at: number }
  | { kind: "error"; event: Extract<AgentStreamEvent, { kind: "error" }>; at: number };

function fmtTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString();
}

export function groupEvents(events: AgentStreamEvent[], live: boolean): RenderRow[] {
  const rows: RenderRow[] = [];
  for (const evt of events) {
    const last = rows[rows.length - 1];
    if (evt.kind === "llm_delta" || evt.kind === "llm_reply_text") {
      if (last?.kind === "llm") {
        last.text += evt.text;
        last.lastAt = Date.now();
      } else {
        rows.push({
          kind: "llm",
          text: evt.text,
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
  return <div className={`flex ${justify}`}>{children}</div>;
}

function UserBubble({ row }: { row: Extract<RenderRow, { kind: "user" }> }) {
  return (
    <BubbleShell side="right">
      <div className="bg-primary/10 max-w-2xl rounded-lg border px-4 py-2" data-message-kind="user">
        <p className="whitespace-pre-wrap text-sm leading-relaxed">{row.text}</p>
        <div className="text-muted-foreground mt-1 text-right text-xs">{fmtTime(row.firstAt)}</div>
      </div>
    </BubbleShell>
  );
}

function AgentBubble({ row }: { row: Extract<RenderRow, { kind: "llm" }> }) {
  return (
    <BubbleShell side="left">
      <div className="bg-card max-w-2xl rounded-lg border px-4 py-2" data-message-kind="agent">
        <p className="whitespace-pre-wrap text-sm leading-relaxed">
          {row.text}
          {row.live && (
            <span className="text-primary ml-0.5 inline-block animate-pulse font-bold">▌</span>
          )}
        </p>
        <div className="text-muted-foreground mt-1 text-xs">{fmtTime(row.firstAt)}</div>
      </div>
    </BubbleShell>
  );
}

function ToolCallRow({ row }: { row: Extract<RenderRow, { kind: "tool_call" }> }) {
  const [expanded, setExpanded] = useState(false);
  const argCount = Object.keys(row.event.args || {}).length;
  return (
    <BubbleShell side="left">
      <Button
        variant="outline"
        size="sm"
        onClick={() => setExpanded((v) => !v)}
        className="max-w-2xl justify-start"
        data-message-kind="tool-call"
        data-action="toggle-tool-call"
      >
        <span className="text-muted-foreground">{expanded ? "▾" : "▸"}</span>
        <span className="text-warning">Called {row.event.tool}</span>
        <span className="text-muted-foreground">
          ({argCount} arg{argCount !== 1 ? "s" : ""})
        </span>
        <span className="text-muted-foreground ml-auto text-xs tabular-nums">
          {fmtTime(row.at)}
        </span>
      </Button>
      {expanded && (
        <pre className="bg-muted/30 mt-1 max-w-2xl rounded-md border p-2 font-mono text-xs leading-relaxed">
          {JSON.stringify(row.event.args, null, 2)}
        </pre>
      )}
    </BubbleShell>
  );
}

function ToolResultRow({ row }: { row: Extract<RenderRow, { kind: "tool_result" }> }) {
  const [expanded, setExpanded] = useState(false);
  const outcome = row.event.outcome;
  return (
    <BubbleShell side="left">
      <Button
        variant="outline"
        size="sm"
        onClick={() => setExpanded((v) => !v)}
        className="max-w-2xl justify-start"
        data-message-kind="tool-result"
        data-action="toggle-tool-result"
      >
        <span className="text-muted-foreground">{expanded ? "▾" : "▸"}</span>
        <span className={outcome.ok ? "text-muted-foreground" : "text-destructive"}>
          Result of {row.event.tool} ({outcome.ok ? "ok" : "error"})
        </span>
        <span className="text-muted-foreground ml-auto text-xs tabular-nums">
          {fmtTime(row.at)}
        </span>
      </Button>
      {expanded && (
        <pre
          className={
            outcome.ok
              ? "bg-muted/30 mt-1 max-w-2xl whitespace-pre-wrap rounded-md border p-2 font-mono text-xs leading-relaxed"
              : "border-destructive/40 bg-destructive/5 text-destructive mt-1 max-w-2xl whitespace-pre-wrap rounded-md border p-2 font-mono text-xs leading-relaxed"
          }
        >
          {outcome.ok ? JSON.stringify(outcome.value, null, 2) : outcome.error}
        </pre>
      )}
    </BubbleShell>
  );
}

function FinalBubble({ row }: { row: Extract<RenderRow, { kind: "final" }> }) {
  return (
    <BubbleShell side="center">
      <div
        className="border-success/40 bg-success/10 w-full max-w-2xl rounded-lg border px-4 py-3"
        data-message-kind="final"
      >
        <div className="text-success mb-2 text-xs font-medium">Final · {row.event.reason}</div>
        <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {stringifyCompact(row.event.value)}
        </pre>
        <div className="text-muted-foreground mt-2 text-xs">{fmtTime(row.at)}</div>
      </div>
    </BubbleShell>
  );
}

function ErrorBubble({ row }: { row: Extract<RenderRow, { kind: "error" }> }) {
  return (
    <BubbleShell side="center">
      <div
        className="border-destructive/40 bg-destructive/10 w-full max-w-2xl rounded-lg border px-4 py-3"
        data-message-kind="error"
      >
        <div className="text-destructive mb-2 text-xs font-medium">Error</div>
        <pre className="text-destructive whitespace-pre-wrap font-mono text-xs leading-relaxed">
          {row.event.message}
        </pre>
        <div className="text-muted-foreground mt-2 text-xs">{fmtTime(row.at)}</div>
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
