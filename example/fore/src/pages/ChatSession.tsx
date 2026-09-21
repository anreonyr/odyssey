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

import type { SessionStatus } from "@/api/types";
import type { StreamPhase } from "@/hooks/useAgentStream";

import { ArrowLeft, ArrowRight, Loader2, Square, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { ChatComposer } from "@/components/ChatComposer";
import { StatusPill } from "@/components/StatusPill";
import { groupEvents, MessageBubble, type RenderRow } from "@/components/StreamEventRow";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useAgentSession } from "@/hooks/useAgentSession";
import { useAgentStream } from "@/hooks/useAgentStream";

const SHELL_HEIGHT = "calc(100vh - 6rem)";

const TERMINAL_STATUSES: ReadonlySet<SessionStatus> = new Set(["Done", "Failed", "Cancelled"]);

export function ChatSession() {
  const { sessionId } = useParams();
  const id = sessionId ?? "";
  const sessionStore = useAgentSession();
  const session = sessionStore.sessions.get(id);
  const stream = useAgentStream();

  useEffect(() => {
    if (!id) return;
    if (session && TERMINAL_STATUSES.has(session.status)) return;
    void stream.start(id);
    return () => stream.cancel();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, session?.status]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const [userMessages, setUserMessages] = useState<Array<{ text: string; firstAt: number }>>([]);

  const rows = useMemo(() => {
    const streamed = groupEvents(stream.events, stream.phase === "streaming");
    const userRows: RenderRow[] = userMessages.map((m) => ({
      kind: "user",
      text: m.text,
      firstAt: m.firstAt,
    }));
    const merged: RenderRow[] = [...userRows, ...streamed];
    const timeOf = (r: RenderRow): number =>
      r.kind === "user" || r.kind === "llm" ? r.firstAt : r.at;
    return merged.sort((a, b) => timeOf(a) - timeOf(b));
  }, [stream.events, stream.phase, userMessages]);

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
            <ArrowLeft className="h-4 w-4" /> Back to chat
          </Link>
        </Button>
        <p className="text-muted-foreground text-sm">Session not found in memory.</p>
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
          setUserMessages([]);
          void stream.start(session.session_id);
        }}
      />

      {stream.error && (
        <div className="border-destructive/40 bg-destructive/10 mb-3 rounded-md border p-3">
          <pre className="text-destructive text-xs">× stream error: {stream.error}</pre>
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
      <span className="text-muted-foreground text-xs">chat / {sessionId.slice(0, 12)}</span>
      <StatusPill phase={phase} />
      <Badge variant="muted">
        <span className="text-muted-foreground">events</span>
        <span className="font-mono tabular-nums">{eventCount}</span>
      </Badge>
      {live && (
        <Badge variant="default">
          <Loader2 className="h-3 w-3 animate-spin" />
          Live
        </Badge>
      )}
      <div className="ml-auto flex items-center gap-2">
        <Button asChild variant="ghost" size="sm">
          <Link to="/chat">
            <ArrowLeft className="h-4 w-4" /> Chat
          </Link>
        </Button>
        <Button size="sm" variant="outline" onClick={onClear}>
          <Trash2 className="h-4 w-4" /> Clear
        </Button>
        <Button size="sm" variant="destructive" onClick={onCancel}>
          <Square className="h-4 w-4" /> Cancel
        </Button>
        <Button asChild size="sm" variant="secondary">
          <Link to={`/agent/sessions/${sessionId}`}>
            Controller <ArrowRight className="h-4 w-4" />
          </Link>
        </Button>
      </div>
    </div>
  );
}

function EmptyStream({ goal }: { goal: string }) {
  return (
    <div className="flex h-full flex-col items-start justify-center gap-3 px-2 py-12">
      <div className="bg-card max-w-md rounded-md border p-4">
        <div className="text-muted-foreground mb-2 text-xs font-medium">Goal</div>
        <p className="text-sm leading-relaxed">{goal}</p>
      </div>
      <p className="text-muted-foreground text-sm">
        Send a message below to start the conversation.
      </p>
    </div>
  );
}

function FloatingComposer(props: {
  sessionId: string;
  phase: StreamPhase;
  onSent: (text: string, sentAt: number) => void;
}) {
  return (
    <div
      data-component="floating-composer"
      className="pointer-events-none fixed bottom-0 left-60 right-0 z-10"
    >
      <div className="pointer-events-auto mx-auto w-full max-w-3xl px-4 pb-6">
        <ChatComposer {...props} />
      </div>
    </div>
  );
}
