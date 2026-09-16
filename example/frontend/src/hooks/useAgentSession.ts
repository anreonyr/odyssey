// Session store. The kernel holds the canonical state; this store
// is a thin React-side cache so the UI can render multiple sessions
// side by side without re-fetching. Every mutation goes back to the
// kernel first; the local cache updates from the response.
//
// `localStorage` is intentionally not used: the kernel's session
// table is in-memory, so persisting the React side would create
// the "list says yes, kernel says no" mismatch a refresh exposes.
// Sessions disappear on reload — same on both sides.

import { useCallback, useState } from "react";
import { agent } from "../api/client";
import type {
  AgentCancelInput,
  AgentHistoryEntry,
  AgentResumeInput,
  AgentStartInput,
  Observation,
  SessionSummary,
} from "../api/types";

export interface SessionEntry extends SessionSummary {
  /** Full local history (each step). Updated after every resume. */
  history: AgentHistoryEntry[];
}

export function useAgentSession() {
  const [sessions, setSessions] = useState<Map<string, SessionEntry>>(new Map());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const upsert = useCallback((entry: SessionEntry) => {
    setSessions((m) => {
      const next = new Map(m);
      next.set(entry.session_id, entry);
      return next;
    });
  }, []);

  const start = useCallback(async (input: AgentStartInput) => {
    setBusy(true);
    setError(null);
    try {
      const r = await agent.start(input);
      upsert({
        session_id: r.session_id,
        goal: input.goal,
        status: "AwaitingObservation",
        history_len: 0,
        allowed_tools: input.allowed_tools ?? [],
        history: [],
      });
      return r.session_id;
    } catch (e) {
      setError((e as Error).message);
      throw e;
    } finally {
      setBusy(false);
    }
  }, [upsert]);

  const resume = useCallback(async (session_id: string, observation: Observation) => {
    setBusy(true);
    setError(null);
    try {
      const input: AgentResumeInput = { session_id, observation };
      const r = await agent.resume(input);
      setSessions((m) => {
        const prev = m.get(session_id);
        if (!prev) return m;
        const next = new Map(m);
        next.set(session_id, {
          ...prev,
          status: r.status,
          history: r.history,
          history_len: r.history.length,
        });
        return next;
      });
      return r;
    } catch (e) {
      setError((e as Error).message);
      throw e;
    } finally {
      setBusy(false);
    }
  }, []);

  const cancel = useCallback(async (session_id: string, path?: string) => {
    setBusy(true);
    setError(null);
    try {
      const input: AgentCancelInput = { session_id, path };
      const r = await agent.cancel(input);
      setSessions((m) => {
        const prev = m.get(session_id);
        if (!prev) return m;
        const next = new Map(m);
        next.set(session_id, {
          ...prev,
          status: "Cancelled",
          history: r.history,
          history_len: r.history.length,
          checkpoint_path: r.checkpoint_path,
        });
        return next;
      });
    } catch (e) {
      setError((e as Error).message);
      throw e;
    } finally {
      setBusy(false);
    }
  }, []);

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      const r = await agent.load({ path });
      upsert({
        session_id: r.session_id,
        // Goal + allowed_tools are not in the load response —
        // they come back via the first resume's history. Until
        // then we show "(loaded from file)".
        goal: "(loaded from file)",
        status: "AwaitingObservation",
        history_len: 0,
        allowed_tools: [],
        history: [],
      });
      return r.session_id;
    } catch (e) {
      setError((e as Error).message);
      throw e;
    } finally {
      setBusy(false);
    }
  }, [upsert]);

  const remove = useCallback((session_id: string) => {
    setSessions((m) => {
      const next = new Map(m);
      next.delete(session_id);
      return next;
    });
  }, []);

  return { sessions, busy, error, start, resume, cancel, load, remove };
}