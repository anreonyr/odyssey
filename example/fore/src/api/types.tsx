// TypeScript mirror of the Rust surface the React app talks to.
//
// The point of having these as hand-written types (not generated)
// is to make the data binding *narrow*. If the kernel adds a new
// field on either side, the wrong type surfaces as a render-time
// undefined or a TS error, not as a silently-dashed column. The
// `tests/frontend.react.mjs` data-binding test is the second
// half of that guarantee: the same field renames that fail TS
// also fail the test's DOM assertions.

import type { ReactNode } from "react";

// ---------- /api/caps (HTTP bridge: list_caps) ----------

export interface CapInfo {
  /** Capability name registered in the cspace. */
  name: string;
  /** Stable capability id. */
  id: string;
  /** True if this cap's `kind == CapKind::Stream`. */
  streaming: boolean;
  /** Per-call wall-clock budget, ms. */
  timeout_ms: number;
}

// ---------- agent_list / agent_describe (read-only observers) ----------

export interface AgentListEntry {
  handle: string;
  live: boolean;
}

export interface AgentListPayload {
  handles: AgentListEntry[];
}

/** A handle that lives behind the agent's binding row. */
export interface AgentHandleDescribeLive {
  handle: string;
  live: true;
  capability: string;
  contract: string;
  name: string;
  namespace: string;
  plugin: string;
  kind: "sync" | "stream";
  streaming: boolean;
  timeout_ms: number;
  calls_per_minute: number;
  operations: Array<"INVOKE" | "ASSIGN" | "REVOKE">;
}

/** A handle whose capability was revoked / never installed. */
export interface AgentHandleDescribeDead {
  handle: string;
  live: false;
  capability: string;
  contract: string;
}

/**
 * The kernel returns one of two shapes from `agent_describe`. The
 * React side discriminates on `live` so a dead row's absent fields
 * stay absent (no undefineds leaked into the tree).
 */
export type AgentHandleDescribe = AgentHandleDescribeLive | AgentHandleDescribeDead;

// ---------- agent_runtime (8 caps) ----------

export interface SessionLimits {
  max_steps?: number;
  max_idle_ms?: number;
  max_session_ms?: number;
}

export interface AgentStartInput {
  goal: string;
  context?: unknown;
  allowed_tools?: string[];
  limits?: SessionLimits;
}

export interface AgentStartPayload {
  session_id: string;
  profile: { session_id: string };
}

/** Three variants — the `kind` field discriminates. */
export type Observation =
  | { kind: "Tick" }
  | { kind: "ToolResult"; tool: string; value: unknown; error: string | null }
  | { kind: "UserReply"; text: string };

export interface AgentResumeInput {
  session_id: string;
  observation: Observation;
}

export interface AgentCancelInput {
  session_id: string;
  /** Optional — if set, the session is checkpointed to this path
   *  before being cancelled. The file can later be loaded via
   *  `agent_load`. */
  path?: string;
}

export interface AgentCancelPayload {
  status: "Cancelled";
  history: AgentHistoryEntry[];
  /** Echo of the input `path`, present only if cancel was
   *  asked to checkpoint. */
  checkpoint_path?: string;
}

export interface AgentPlanInput {
  goal: string;
  tools?: string[];
}

export interface AgentPlanPayload {
  steps: AgentPlanStep[];
}

/**
 * One row of the history the kernel returns from `agent_resume`.
 * The agent splits `LlmReply::Text` into its own kind and tags the
 * embedded tool_call / final differently from the top-level
 * `tool_call` / `final` step — that split is what `StepRow`
 * renders, and what the rename-catcher test depends on.
 */
export type AgentHistoryEntry =
  | { kind: "llm_text"; text: string }
  | { kind: "llm_tool_call"; tool: string; args: unknown }
  | { kind: "llm_final"; value: unknown; reason: string }
  | { kind: "tool_call"; tool: string; args: unknown }
  | { kind: "tool_result"; tool: string; outcome: AgentOutcome }
  | { kind: "final"; value: unknown; reason: string };

/**
 * The agent's pure-plan response also returns history-shaped
 * entries (a single `llm_text` from the mock LLM in tests). The
 * shape is the same.
 */
export type AgentPlanStep = AgentHistoryEntry;

export interface AgentLoadInput {
  path: string;
}

export interface AgentLoadPayload {
  session_id: string;
}

export interface AgentMemoryRecallInput {
  query: string;
  top_k?: number;
  filter_tags?: string[];
}

export interface AgentMemoryHit {
  content: string;
  tags: string[];
  score: number;
  created_at: number;
}

export interface AgentMemoryRecallPayload {
  hits: AgentMemoryHit[];
}

export interface AgentMemoryRecordInput {
  content: string;
  tags?: string[];
}

export interface AgentMemoryRecordPayload {
  id: string;
}

// ---------- /api/checkpoints (HTTP bridge: list_checkpoints) ----------
//
// One row per checkpoint file the agent_runtime has written to
// disk. The Rust side tracks these from the `cancel` cap's
// optional `path` argument; this endpoint just enumerates the
// files it knows about.

export interface CheckpointInfo {
  /** Absolute path on the bridge's host. */
  path: string;
  /** Session id the checkpoint belongs to. */
  session_id: string;
  /** Unix epoch (seconds) when the checkpoint was written. */
  saved_at: number;
  /** File size in bytes; useful for the list UI. */
  size: number;
}

// ---------- /api/invoke + /api/stream transport ----------

export interface InvokeRequest {
  capability: string;
  input: unknown;
}

export interface InvokeResponse {
  capability: string;
  value: unknown;
}

export interface InvokeError {
  error: string;
}

// ---------- Streaming (agent_stream) ----------

/**
 * Mirrors the Rust `AgentEvent` enum's serialisation into SSE
 * `data:` frames. The kinds use snake_case because that's what
 * the bridge writes — keeping the case the same as the wire
 * format means the rename-catcher test is unambiguous.
 */
export type AgentStreamEvent =
  | { kind: "llm_reply_text"; text: string }
  | { kind: "llm_delta"; text: string }
  | { kind: "tool_call"; tool: string; args: unknown }
  | { kind: "tool_result"; tool: string; outcome: AgentOutcome }
  | { kind: "final"; value: unknown; reason: string }
  | { kind: "error"; message: string };

export type AgentOutcome = { ok: true; value: unknown } | { ok: false; error: string };

export type FinalReason = "Goal" | "MaxSteps" | "ToolFailure";

export type SessionStatus = "Running" | "AwaitingObservation" | "Done" | "Failed" | "Cancelled";

export interface AgentResumePayload {
  /** The current step. */
  step: AgentHistoryEntry;
  history: AgentHistoryEntry[];
  status: SessionStatus;
}

// ---------- UI helpers ----------

export interface SessionSummary {
  session_id: string;
  goal: string;
  status: SessionStatus;
  history_len: number;
  allowed_tools: string[];
  /** True if a checkpoint file exists for this session. */
  checkpoint_path?: string;
}

export interface ReactChildren {
  children: ReactNode;
}
