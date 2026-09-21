// Session store. The kernel holds the canonical state; this store
// is a thin React-side cache so the UI can render multiple sessions
// side by side without re-fetching. Every mutation goes back to the
// kernel first; the local cache updates from the response.
//
// `localStorage` is intentionally not used: the kernel's session
// table is in-memory, so persisting the React side would create
// the "list says yes, kernel says no" mismatch a refresh exposes.
// Sessions disappear on reload — same on both sides.
//
// The state is held by a single `SessionProvider` mounted in
// `AppShell`; every consumer of `useAgentSession()` reads the
// SAME Map. Without the provider, the hook throws — there is no
// per-component fallback, because per-component state silently
// breaks the launcher → detail-view flow (the launcher adds a
// session to its own Map, then the detail view's separate Map
// can't see it). Context makes the data binding obvious.

import type {
  AgentCancelInput,
  AgentHistoryEntry,
  AgentResumeInput,
  AgentStartInput,
  Observation,
  SessionSummary,
} from "../api/types";

import { createContext, useCallback, useContext, useMemo, useState } from "react";

import { agent } from "../api/client";

export interface SessionEntry extends SessionSummary {
  /** Full local history (each step). Updated after every resume. */
  history: AgentHistoryEntry[];
}

export interface AgentSessionStore {
  sessions: Map<string, SessionEntry>;
  busy: boolean;
  error: string | null;
  start: (input: AgentStartInput) => Promise<string>;
  resume: (session_id: string, observation: Observation) => Promise<unknown>;
  cancel: (session_id: string, path?: string) => Promise<void>;
  load: (path: string) => Promise<string>;
  remove: (session_id: string) => void;
}

const SessionContext = createContext<AgentSessionStore | null>(null);

export function SessionProvider({ children }: { children: React.ReactNode }) {
  const store = useSessionStore();
  return <SessionContext.Provider value={store}>{children}</SessionContext.Provider>;
}

export function useAgentSession(): AgentSessionStore {
  const store = useContext(SessionContext);
  if (!store) {
    throw new Error(
      "useAgentSession must be called inside <SessionProvider>. " +
        "AppShell mounts the provider; custom roots must wrap their tree with it.",
    );
  }
  return store;
}

function useSessionStore(): AgentSessionStore {
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

  const start = useCallback(
    async (input: AgentStartInput) => {
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
    },
    [upsert],
  );

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
      const input: AgentCancelInput = path !== undefined ? { session_id, path } : { session_id };
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
          ...(r.checkpoint_path !== undefined && { checkpoint_path: r.checkpoint_path }),
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

  const load = useCallback(
    async (path: string) => {
      setBusy(true);
      setError(null);
      try {
        const r = await agent.load({ path });
        upsert({
          session_id: r.session_id,
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
    },
    [upsert],
  );

  const remove = useCallback((session_id: string) => {
    setSessions((m) => {
      const next = new Map(m);
      next.delete(session_id);
      return next;
    });
  }, []);

  return useMemo(
    () => ({ sessions, busy, error, start, resume, cancel, load, remove }),
    [sessions, busy, error, start, resume, cancel, load, remove],
  );
}
