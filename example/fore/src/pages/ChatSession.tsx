// Streaming chat view (`/chat/:sessionId`). Subscribes to the
// session's `agent_stream` broadcast and renders events as they
// arrive. The session lifecycle (start / cancel) is owned by
// `useAgentSession`; the stream itself is a separate hook so
// the read path doesn't fight the write path.
//
// The page carries the brutalist identity: oversized mono
// header, ASCII rules, sharp-cornered panels with thick
// borders, mono everywhere. The event log is a vertical stack
// of discriminated panels — each tool call and result gets its
// own bordered row, consecutive LLM deltas collapse into a
// single running bubble with a blinking caret.

import { ArrowLeft, ArrowRight, Square, Trash2, Loader2 } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";

import { ChatComposer } from "../components/ChatComposer";
import { StatusPill } from "../components/StatusPill";
import { groupEvents, StreamEventRow } from "../components/StreamEventRow";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent } from "../components/ui/card";
import { useAgentSession } from "../hooks/useAgentSession";
import { useAgentStream } from "../hooks/useAgentStream";

export function ChatSession() {
  const { sessionId } = useParams();
  const id = sessionId ?? "";
  const navigate = useNavigate();
  const sessionStore = useAgentSession();
  const session = sessionStore.sessions.get(id);
  const stream = useAgentStream();

  // Subscribe on mount / session change; the hook owns cleanup.
  useEffect(() => {
    if (!id) return;
    void stream.start(id);
    return () => stream.cancel();
    // stream object identity is stable across renders for a given
    // mount — we only want to re-subscribe when the URL changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const rows = useMemo(
    () => groupEvents(stream.events, stream.phase === "streaming"),
    [stream.events, stream.phase],
  );

  // Auto-scroll to bottom on new events; the user can scroll up
  // freely between bursts because we only snap when at-bottom.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (near) el.scrollTop = el.scrollHeight;
  }, [rows.length]);

  if (!session) {
    return (
      <div className="space-y-3">
        <Button asChild variant="ghost" size="sm">
          <Link to="/chat">
            <ArrowLeft className="h-3.5 w-3.5" /> back to chat
          </Link>
        </Button>
        <Card className="border-2">
          <CardContent className="space-y-3 p-6 text-sm">
            <div className="text-destructive font-mono text-xs uppercase tracking-widest">
              × session not found
            </div>
            <p>
              session <code className="bg-muted px-1.5 py-0.5">{id}</code> is not in the in-memory
              store.
            </p>
            <p className="text-muted-foreground text-xs leading-relaxed">
              sessions live only in kernel memory — they do not survive a page reload, a backend
              restart, or opening this URL in a new tab. the most common cause is reloading after
              starting the session.
            </p>
            <div className="flex items-center gap-2 pt-1">
              <Button asChild size="sm" variant="success">
                <Link to="/chat">
                  start a new session
                  <ArrowRight className="h-3.5 w-3.5" />
                </Link>
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  const live = stream.phase === "streaming" || stream.phase === "connecting";

  return (
    <div className="space-y-4">
      <PageHeader
        sessionId={session.session_id}
        goal={session.goal}
        phase={stream.phase}
        eventCount={stream.events.length}
        live={live}
        onCancel={async () => {
          await sessionStore.cancel(session.session_id);
          stream.cancel();
        }}
        onClear={() => {
          stream.clear();
          void stream.start(session.session_id);
        }}
      />

      {stream.error && (
        <Card className="border-destructive/40 border-2">
          <CardContent className="p-3">
            <pre className="text-destructive font-mono text-[11px]">
              × stream error: {stream.error}
            </pre>
          </CardContent>
        </Card>
      )}

      <EventLog rows={rows} scrollRef={scrollRef} empty={stream.events.length === 0} />

      <ChatComposer
        sessionId={session.session_id}
        phase={stream.phase}
        onSent={() => {
          // The stream will surface the agent's reply as it
          // arrives — no local state to update.
        }}
        onCancel={async () => {
          await sessionStore.cancel(session.session_id);
          stream.cancel();
          navigate("/chat");
        }}
      />
    </div>
  );
}

// ---------------- Sub-components ----------------

function PageHeader({
  sessionId,
  goal,
  phase,
  eventCount,
  live,
  onCancel,
  onClear,
}: {
  sessionId: string;
  goal: string;
  phase: import("../hooks/useAgentStream").StreamPhase;
  eventCount: number;
  live: boolean;
  onCancel: () => void;
  onClear: () => void;
}) {
  return (
    <div className="space-y-3">
      <div className="flex items-baseline gap-4">
        <h1 className="text-foreground font-mono text-6xl font-bold leading-none tracking-tight md:text-8xl">
          A G E N T
        </h1>
        <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
          ── /chat/{sessionId.slice(0, 12)}
        </span>
      </div>

      <pre className="text-muted-foreground font-mono text-[10px] leading-tight">
        {`──────────────────────────────────────────────────────────────────────────────
  STATUS: ${phase.toUpperCase().padEnd(8)} · EVENTS: ${String(eventCount).padStart(4)} · LIVE: ${live ? "yes" : "no "}
──────────────────────────────────────────────────────────────────────────────`}
      </pre>

      <div className="flex flex-wrap items-center gap-3">
        <Button asChild variant="ghost" size="sm">
          <Link to="/chat">
            <ArrowLeft className="h-3.5 w-3.5" /> chat
          </Link>
        </Button>
        <StatusPill phase={phase} />
        <Badge variant="muted">
          <span className="text-muted-foreground">events</span>
          <span className="text-foreground tabular-nums">{eventCount}</span>
        </Badge>
        {live && (
          <Badge variant="stream">
            <Loader2 className="h-3 w-3 animate-spin" />
            live
          </Badge>
        )}
        <div className="ml-auto flex items-center gap-2">
          <Button size="sm" variant="outline" onClick={onClear} data-action="reconnect">
            <Trash2 className="h-3.5 w-3.5" /> clear &amp; reconnect
          </Button>
          <Button size="sm" variant="destructive" onClick={onCancel} data-action="cancel-session">
            <Square className="h-3.5 w-3.5" /> cancel
          </Button>
          <Button asChild size="sm" variant="secondary">
            <Link to={`/agent/${sessionId}`}>
              controller <ArrowRight className="h-3.5 w-3.5" />
            </Link>
          </Button>
        </div>
      </div>

      <Card className="border-2">
        <CardContent className="p-3">
          <div className="text-muted-foreground mb-1 font-mono text-[10px] uppercase tracking-widest">
            ── goal
          </div>
          <p className="font-mono text-sm">{goal}</p>
        </CardContent>
      </Card>
    </div>
  );
}

function EventLog({
  rows,
  scrollRef,
  empty,
}: {
  rows: ReturnType<typeof groupEvents>;
  scrollRef: React.RefObject<HTMLDivElement>;
  empty: boolean;
}) {
  return (
    <Card className="border-2">
      <CardContent className="p-0">
        <div className="border-border bg-card/60 border-b-2 px-4 py-2">
          <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
            ╔ STREAM
          </span>
        </div>

        <div
          ref={scrollRef}
          data-component="event-log"
          className="max-h-[60vh] space-y-2 overflow-y-auto p-3"
        >
          {empty ? (
            <div className="text-muted-foreground py-12 text-center font-mono text-xs">
              <pre className="leading-tight">
                {`┌─────────────────────────────────────────────────────────────┐
│  no events yet                                              │
│  ── the stream will populate as the agent advances         │
│  ── llm_delta, tool_call, tool_result, final, error         │
└─────────────────────────────────────────────────────────────┘`}
              </pre>
            </div>
          ) : (
            rows.map((row, idx) => <StreamEventRow key={idx} row={row} step={idx} />)
          )}
        </div>
      </CardContent>
    </Card>
  );
}
