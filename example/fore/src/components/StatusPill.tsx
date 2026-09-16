// Brutalist status pill for the chat surface. All caps, mono,
// tracking-wide, brackets as decoration. Differentiation via
// typography — no rounded backgrounds, no soft borders.

import type { StreamPhase } from "../hooks/useAgentStream";

import { cn } from "../lib/utils";

const PHASE_STYLES: Record<StreamPhase, { label: string; className: string }> = {
  idle: {
    label: "IDLE",
    className: "border-border bg-card text-muted-foreground",
  },
  connecting: {
    label: "CONN",
    className: "border-warning/50 bg-warning/10 text-warning",
  },
  streaming: {
    label: "STREAM",
    className: "border-success/50 bg-success/10 text-success",
  },
  done: {
    label: "DONE",
    className: "border-border bg-muted text-foreground",
  },
  error: {
    label: "ERROR",
    className: "border-destructive/50 bg-destructive/10 text-destructive",
  },
};

export function StatusPill({ phase }: { phase: StreamPhase }) {
  const s = PHASE_STYLES[phase];
  return (
    <span
      data-phase={phase}
      className={cn(
        "inline-flex items-center gap-1.5 border-2 px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest",
        s.className,
      )}
    >
      <span className="font-bold">[</span>
      <span>{s.label}</span>
      <span className="font-bold">]</span>
    </span>
  );
}
