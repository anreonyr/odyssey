// Stream-phase pill — a Badge keyed to the agent_stream
// lifecycle phase (idle / connecting / streaming / done /
// error). Used in the ChatSession compact header.

import type { StreamPhase } from "@/hooks/useAgentStream";

import { Badge } from "@/components/ui/badge";

const PHASE_VARIANT: Record<
  StreamPhase,
  "muted" | "warning" | "default" | "muted" | "destructive"
> = {
  idle: "muted",
  connecting: "warning",
  streaming: "default",
  done: "muted",
  error: "destructive",
};

const PHASE_LABEL: Record<StreamPhase, string> = {
  idle: "Idle",
  connecting: "Connecting",
  streaming: "Streaming",
  done: "Done",
  error: "Error",
};

export function StatusPill({ phase }: { phase: StreamPhase }) {
  return (
    <Badge variant={PHASE_VARIANT[phase]} data-phase={phase}>
      {PHASE_LABEL[phase]}
    </Badge>
  );
}
