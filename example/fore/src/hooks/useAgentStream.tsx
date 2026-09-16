// Subscribe to a session's `agent_stream` SSE feed and surface
// events as a typed array. The kernel broadcasts `AgentEvent`s
// to all subscribers — LLM token deltas, tool calls, tool
// results, the terminal `final` / `error`. This hook is the
// chat surface's read path; write paths go through
// `agent.start` / `agent.cancel` like any other session.
//
// Parsing is best-effort: chunks that aren't valid JSON for an
// `AgentStreamEvent` are dropped on the floor (the kernel can
// in principle emit a trailing newline or an SSE keepalive
// frame). The chat view never sees them.

import type { AgentStreamEvent } from "../api/types";

import { useCallback, useEffect, useRef, useState } from "react";

import { client } from "../api/client";

export type StreamPhase = "idle" | "connecting" | "streaming" | "done" | "error";

export interface AgentStreamState {
  events: AgentStreamEvent[];
  phase: StreamPhase;
  error: string | null;
}

export function useAgentStream() {
  const [state, setState] = useState<AgentStreamState>({
    events: [],
    phase: "idle",
    error: null,
  });
  const abortRef = useRef<AbortController | null>(null);

  const start = useCallback(async (sessionId: string) => {
    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setState({ events: [], phase: "connecting", error: null });

    try {
      let firstChunk = true;
      for await (const chunk of client.stream("agent_stream", { session_id: sessionId })) {
        if (ctrl.signal.aborted) break;
        let evt: AgentStreamEvent;
        try {
          evt = JSON.parse(chunk) as AgentStreamEvent;
        } catch {
          continue;
        }
        setState((s) => ({
          events: [...s.events, evt],
          phase: firstChunk ? "streaming" : s.phase,
          error: null,
        }));
        firstChunk = false;
      }
      setState((s) => ({ ...s, phase: ctrl.signal.aborted ? "idle" : "done" }));
    } catch (e) {
      if (ctrl.signal.aborted) {
        setState((s) => ({ ...s, phase: "idle" }));
      } else {
        setState((s) => ({ ...s, phase: "error", error: (e as Error).message }));
      }
    }
  }, []);

  const cancel = useCallback(() => {
    abortRef.current?.abort();
    setState((s) => ({ ...s, phase: "idle" }));
  }, []);

  const clear = useCallback(() => {
    setState({ events: [], phase: "idle", error: null });
  }, []);

  useEffect(() => () => abortRef.current?.abort(), []);

  return { ...state, start, cancel, clear };
}
