// Subscribes to a streaming capability and surfaces chunks as an
// array. The agent's `agent_stream` emits typed JSON events; the
// generic stream cap (`streaming_echo`) emits plain strings.
//
// The hook is intentionally generic: it does not parse the chunk
// payload, so the caller decides how to render. `agent_stream`
// passes raw JSON strings; the consuming component dispatches on
// the `kind` field of each parsed event.

import { useCallback, useEffect, useRef, useState } from "react";

import { client } from "../api/client";

export interface StreamState {
  chunks: string[];
  done: boolean;
  error: string | null;
  busy: boolean;
}

export function useStream(capability: string, buildInput: () => unknown) {
  const [state, setState] = useState<StreamState>({
    chunks: [],
    done: false,
    error: null,
    busy: false,
  });
  const abortRef = useRef<AbortController | null>(null);

  const start = useCallback(async () => {
    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setState({ chunks: [], done: false, error: null, busy: true });
    try {
      for await (const chunk of client.stream(capability, buildInput())) {
        if (ctrl.signal.aborted) break;
        setState((s) => ({ ...s, chunks: [...s.chunks, chunk] }));
      }
      setState((s) => ({ ...s, done: true, busy: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: (e as Error).message, busy: false }));
    }
  }, [capability, buildInput]);

  const cancel = useCallback(() => {
    abortRef.current?.abort();
    setState((s) => ({ ...s, busy: false }));
  }, []);

  useEffect(() => () => abortRef.current?.abort(), []);

  return { ...state, start, cancel };
}
