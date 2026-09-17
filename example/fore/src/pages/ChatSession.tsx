// Streaming chat view (`/chat/:sessionId`). Two-sided bubble
// chat (user right, agent left) with a floating composer.
//
// The full execution trace (tool calls, results) is collapsed
// into one-line summaries that expand on click — the user sees
// the conversation, not the agent's internals. Final and error
// events break out of the bubble layout and span the column
// for emphasis.
//
// The outer container uses an explicit `calc(100vh - 6rem)`
// height because the `h-full` chain through the AppShell main
// wrapper doesn't reliably resolve against main's flex-1
// height (a flex algorithm height isn't a "definite" height
// for percentage resolution in all engines).

import type { SessionStatus } from "../api/types";
import type { StreamPhase } from "../hooks/useAgentStream";

import { ArrowLeft, ArrowRight, Loader2, Square, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { ChatComposer } from "../components/ChatComposer";
import { StatusPill } from "../components/StatusPill";
import { groupEvents, MessageBubble, type RenderRow } from "../components/StreamEventRow";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { useAgentSession } from "../hooks/useAgentSession";
import { useAgentStream } from "../hooks/useAgentStream";

const SHELL_HEIGHT = "calc(100vh - 6rem)";

// Sessions in these states have no live stream — the kernel
// rejects `agent_stream` subscription with `session ... is
// terminal (Done|Failed|Cancelled)`. Skip the subscribe call
// so users revisiting a finished session don't see a confusing
// red banner for an expected condition.
const TERMINAL_STATUSES: ReadonlySet<SessionStatus> = new Set(["Done", "Failed", "Cancelled"]);

export function ChatSession() {
  const { sessionId } = useParams();
  const id = sessionId ?? "";
  const sessionStore = useAgentSession();
  const session = sessionStore.sessions.get(id);
  const stream = useAgentStream();

  // Subscribe on mount / session change; the hook owns cleanup.
  // Skip subscription for terminal sessions — the kernel
  // rejects agent_stream on Done/Failed/Cancelled and the
  // resulting error banner is confusing for users just trying
  // to read past history.
  useEffect(() => {
    if (!id) return;
    if (session && TERMINAL_STATUSES.has(session.status)) return;
    void stream.start(id);
    return () => stream.cancel();
    // stream object identity is stable across renders for a given
    // mount — we only want to re-subscribe when the URL changes
    // or the session's terminal-ness flips.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, session?.status]);

  const scrollRef = useRef<HTMLDivElement>(null);

  // The kernel doesn't echo UserReply observations back through
  // agent_stream, so we splice each submitted message in with
  // its click-time timestamp so the user's own text shows up
  // in the log alongside the agent's replies.
  const [userMessages, setUserMessages] = useState<Array<{ text: string; firstAt: number }>>([]);

  const rows = useMemo(() => {
    const streamed = groupEvents(stream.events, stream.phase === "streaming");
    const userRows: RenderRow[] = userMessages.map((m) => ({
      kind: "user",
      text: m.text,
      firstAt: m.firstAt,
    }));
    const merged: RenderRow[] = [...userRows, ...streamed];
    // user / llm rows carry firstAt; tool_call / tool_result /
    // final / error carry at. Coalesce to one key for the merge
    // sort so the user's text lands before the agent's reply
    // even if the agent's first event arrives before the resume
    // request resolves.
    const timeOf = (r: RenderRow): number =>
      r.kind === "user" || r.kind === "llm" ? r.firstAt : r.at;
    return merged.sort((a, b) => timeOf(a) - timeOf(b));
  }, [stream.events, stream.phase, userMessages]);

  // Auto-scroll only when the user is already near the bottom;
  // they can scroll up freely to read history without us yanking
  // them back down.
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
        <p className="text-muted-foreground text-xs">session not found in memory.</p>
      </div>
    );
  }

  const live = stream.phase === "streaming" || stream.phase === "connecting";

  return (
    <div className="flex min-h-0 flex-col" style={{ height: SHELL_HEIGHT }}>
      <CompactHeader
        sessionId={session.session_id}
        phase={stream.phase}
        eventCount={stream.events.length}
        live={live}
        onCancel={async () => {
          await sessionStore.cancel(session.session_id);
          stream.cancel();
        }}
        onClear={() => {
          stream.clear();
          // Wipe local user rows so reconnect doesn't replay
          // stale messages from the prior conversation.
          setUserMessages([]);
          void stream.start(session.session_id);
        }}
      />

      {stream.error && (
        <div className="border-destructive/40 bg-destructive/10 mb-3 border-2 p-3">
          <pre className="text-destructive font-mono text-[11px]">
            × stream error: {stream.error}
          </pre>
        </div>
      )}

      <div
        ref={scrollRef}
        data-component="event-log"
        className="min-h-0 flex-1 overflow-y-auto px-2 pb-40"
      >
        {stream.events.length === 0 && userMessages.length === 0 ? (
          <EmptyStream goal={session.goal} />
        ) : (
          <div className="mx-auto max-w-3xl space-y-3 py-4">
            {rows.map((row, idx) => (
              <MessageBubble key={idx} row={row} />
            ))}
          </div>
        )}
      </div>

      <FloatingComposer
        sessionId={session.session_id}
        phase={stream.phase}
        onSent={(text, sentAt) => {
          setUserMessages((prev) => [...prev, { text, firstAt: sentAt }]);
        }}
      />
    </div>
  );
}

// ---------------- Sub-components ----------------

function CompactHeader({
  sessionId,
  phase,
  eventCount,
  live,
  onCancel,
  onClear,
}: {
  sessionId: string;
  phase: StreamPhase;
  eventCount: number;
  live: boolean;
  onCancel: () => void;
  onClear: () => void;
}) {
  return (
    <div className="mb-3 flex flex-wrap items-center gap-3 border-b pb-3">
      <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
        chat / {sessionId.slice(0, 12)}
      </span>
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
        <Button asChild variant="ghost" size="sm">
          <Link to="/chat">
            <ArrowLeft className="h-3.5 w-3.5" /> chat
          </Link>
        </Button>
        <Button size="sm" variant="outline" onClick={onClear} data-action="reconnect">
          <Trash2 className="h-3.5 w-3.5" /> clear
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
  );
}

function EmptyStream({ goal }: { goal: string }) {
  return (
    <div className="flex h-full flex-col items-start justify-center gap-3 px-2 py-12">
      <div className="bg-card border-border max-w-md border-2 p-4">
        <div className="text-muted-foreground mb-2 font-mono text-[10px] font-bold uppercase tracking-widest">
          ≡ goal
        </div>
        <p className="font-mono text-sm leading-relaxed">{goal}</p>
      </div>
      <p className="text-muted-foreground font-mono text-xs">
        send a message below to start the conversation
      </p>
    </div>
  );
}

/** Floating composer pinned to the bottom of the viewport.
 *  Aligned to the same `max-w-3xl` column as the message list
 *  above. `pointer-events-none` on the wrapper lets clicks fall
 *  through the empty padding beneath the composer so the chat
 *  log remains scrollable from anywhere on the page. */
function FloatingComposer(props: {
  sessionId: string;
  phase: StreamPhase;
  onSent: (text: string, sentAt: number) => void;
}) {
  return (
    <div
      data-component="floating-composer"
      className="pointer-events-none fixed bottom-0 left-56 right-0 z-10"
    >
      <div className="pointer-events-auto mx-auto w-full max-w-3xl px-4 pb-6">
        <ChatComposer {...props} />
      </div>
    </div>
  );
}
