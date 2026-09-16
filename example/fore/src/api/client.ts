// HTTP client for the odyssey bridge.
//
// Three endpoints back the whole UI:
//   GET  /api/caps    — flat capability list (no rights, no ops)
//   POST /api/invoke  — sync dispatch
//   POST /api/stream  — SSE for streaming caps
//
// `agent_*` capabilities are reached via /api/invoke like any other.
// That keeps the agent panel honest: it observes capabilities via
// the same surface that does work, instead of a parallel HTTP API.

import type {
  AgentCancelInput,
  AgentCancelPayload,
  AgentHandleDescribe,
  AgentListPayload,
  AgentLoadInput,
  AgentLoadPayload,
  AgentMemoryRecallInput,
  AgentMemoryRecallPayload,
  AgentMemoryRecordInput,
  AgentMemoryRecordPayload,
  AgentPlanInput,
  AgentPlanPayload,
  AgentResumeInput,
  AgentResumePayload,
  AgentStartInput,
  AgentStartPayload,
  CapInfo,
  CheckpointInfo,
  InvokeError,
  InvokeRequest,
  InvokeResponse,
} from "./types";

class BridgeError extends Error {
  readonly payload: InvokeError;
  constructor(payload: InvokeError) {
    super(payload.error);
    this.payload = payload;
    this.name = "BridgeError";
  }
}

async function post<TReq, TRes>(
  path: string,
  body: TReq,
): Promise<TRes> {
  const r = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await r.text();
  // The bridge answers 200 for both success and typed errors; the
  // envelope `value` carries the success shape, `error` carries the
  // stringified `CapabilityError`. Treat any non-2xx as fatal so the
  // UI doesn't show a half-parsed body.
  if (!r.ok) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch {
      throw new Error(`${path}: ${r.status} ${text}`);
    }
    const err = (parsed as InvokeError).error ?? `${path}: ${r.status}`;
    throw new BridgeError({ error: err });
  }
  return JSON.parse(text) as TRes;
}

export const client = {
  async listCaps(): Promise<CapInfo[]> {
    const r = await fetch("/api/caps");
    if (!r.ok) throw new Error(`/api/caps: ${r.status}`);
    return r.json();
  },

  /**
   * List the checkpoint files the bridge knows about. Each entry
   * comes from the agent's tracked `checkpoint_path` records; the
   * Rust side filters out missing files before answering.
   */
  async listCheckpoints(): Promise<CheckpointInfo[]> {
    const r = await fetch("/api/checkpoints");
    if (!r.ok) throw new Error(`/api/checkpoints: ${r.status}`);
    return r.json();
  },

  invoke<TRes = unknown>(capability: string, input: unknown): Promise<TRes> {
    const body: InvokeRequest = { capability, input };
    return post<InvokeRequest, InvokeResponse>("/api/invoke", body).then(
      (r) => r.value as TRes,
    );
  },

  /**
   * Open an SSE stream and yield one chunk per `event: chunk` /
   * `done` / `error` line. The parser is intentionally dumb — the
   * kernel emits one JSON-serialised value per chunk and a bare
   * empty `data:` for `done` — and tolerant of multi-line `data:`
   * fields (the bridge concatenates them per the SSE spec).
   */
  async *stream(capability: string, input: unknown): AsyncGenerator<string, void, void> {
    const r = await fetch("/api/stream", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ capability, input } satisfies InvokeRequest),
    });
    if (!r.ok || !r.body) {
      const text = await r.text();
      throw new Error(`/api/stream: ${r.status} ${text}`);
    }
    const reader = r.body.getReader();
    const dec = new TextDecoder();
    let buf = "";
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      let idx: number;
      while ((idx = buf.indexOf("\n\n")) !== -1) {
        const frame = buf.slice(0, idx);
        buf = buf.slice(idx + 2);
        let event = "message";
        let data = "";
        for (const line of frame.split("\n")) {
          if (line.startsWith("event:")) event = line.slice(6).trim();
          else if (line.startsWith("data:")) data += line.slice(5).trim();
        }
        if (event === "done") return;
        if (event === "error") throw new Error(data);
        yield data;
      }
    }
  },
};

// ---------- Typed wrappers for the agent caps ----------
//
// Each helper takes the typed input shape and unwraps the
// `{ capability, value }` envelope the bridge returns. The names
// match the cap names minted by the agent_runtime builtin.

export const agent = {
  list: (): Promise<AgentListPayload> => client.invoke("agent_list", {}),
  describe: (handle: string): Promise<AgentHandleDescribe> =>
    client.invoke("agent_describe", { handle }),

  start: (input: AgentStartInput): Promise<AgentStartPayload> =>
    client.invoke<AgentStartPayload>("agent_start", input),
  resume: (input: AgentResumeInput): Promise<AgentResumePayload> =>
    client.invoke<AgentResumePayload>("agent_resume", input),
  cancel: (input: AgentCancelInput): Promise<AgentCancelPayload> =>
    client.invoke<AgentCancelPayload>("agent_cancel", input),
  plan: (input: AgentPlanInput): Promise<AgentPlanPayload> =>
    client.invoke<AgentPlanPayload>("agent_plan", input),
  load: (input: AgentLoadInput): Promise<AgentLoadPayload> =>
    client.invoke<AgentLoadPayload>("agent_load", input),

  memoryRecall: (input: AgentMemoryRecallInput): Promise<AgentMemoryRecallPayload> =>
    client.invoke<AgentMemoryRecallPayload>("agent_memory_recall", input),
  memoryRecord: (input: AgentMemoryRecordInput): Promise<AgentMemoryRecordPayload> =>
    client.invoke<AgentMemoryRecordPayload>("agent_memory_record", input),
};

export { BridgeError };