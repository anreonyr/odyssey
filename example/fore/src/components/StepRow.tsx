// One row in the agent history. Discriminates on `kind` so
// each history variant renders its own shape (text vs tool
// call vs tool result vs final). The colour-coded left bar
// keeps the timeline scannable.

import type { AgentHistoryEntry } from "@/api/types";

import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

const KIND_STYLES: Record<AgentHistoryEntry["kind"], { bar: string; label: string }> = {
  llm_text: { bar: "bg-primary", label: "LLM" },
  llm_tool_call: { bar: "bg-warning", label: "LLM → tool" },
  llm_final: { bar: "bg-success", label: "LLM final" },
  tool_call: { bar: "bg-warning", label: "Tool call" },
  tool_result: { bar: "bg-muted-foreground", label: "Tool result" },
  final: { bar: "bg-success", label: "Final" },
};

export function StepRow({ step }: { step: AgentHistoryEntry }) {
  const style = KIND_STYLES[step.kind];

  return (
    <div className="flex items-start gap-3">
      <span aria-hidden className={cn("mt-1 h-3 w-0.5 shrink-0 rounded-r", style.bar)} />
      <div className="min-w-0 flex-1">
        <div className="mb-1 flex items-center gap-2">
          <Badge variant="muted">{style.label}</Badge>
        </div>
        <Body step={step} />
      </div>
    </div>
  );
}

function Body({ step }: { step: AgentHistoryEntry }) {
  switch (step.kind) {
    case "llm_text":
      return (
        <pre className="bg-muted/30 overflow-x-auto rounded-md border p-2 font-mono text-xs leading-relaxed">
          {step.text}
        </pre>
      );
    case "llm_final":
    case "final":
      return (
        <pre className="bg-muted/30 overflow-x-auto rounded-md border p-2 font-mono text-xs">
          {typeof step.value === "string" ? step.value : JSON.stringify(step, null, 2)}
        </pre>
      );
    case "llm_tool_call":
    case "tool_call":
      return (
        <div className="flex items-center gap-2 text-sm">
          <code className="font-mono">{step.tool}</code>
          <pre className="bg-muted/30 flex-1 overflow-x-auto rounded-md border p-2 font-mono text-xs">
            {JSON.stringify(step.args, null, 2)}
          </pre>
        </div>
      );
    case "tool_result":
      return (
        <div className="flex items-center gap-2 text-sm">
          <code className="font-mono">{step.tool}</code>
          {step.outcome.ok ? (
            <Badge variant="success">ok</Badge>
          ) : (
            <Badge variant="destructive">error</Badge>
          )}
          <pre className="bg-muted/30 flex-1 overflow-x-auto rounded-md border p-2 font-mono text-xs">
            {JSON.stringify(step.outcome, null, 2)}
          </pre>
        </div>
      );
  }
}
