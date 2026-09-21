// Composer for sending UserReply observations into a running
// session. Submitting triggers `agent_resume` with
// `{ kind: "UserReply", text }`; the kernel processes the
// observation and emits the resulting steps through the same
// `agent_stream` broadcast this page is already subscribed to.
//
// On successful submission the parent receives
// `onSent(text, sentAt)` where `sentAt` is captured at click
// time so the parent can splice the user row into the event
// log ahead of any agent response that started arriving while
// the resume request was in flight.
//
// Disabled while the stream is connecting or already
// streaming — the user can't interject while the agent is
// mid-turn. Stop / cancel lives in the page header, not here,
// to keep this surface focused on "send a message".

import type { StreamPhase } from "@/hooks/useAgentStream";

import { Loader2, Send } from "lucide-react";
import { useRef, useState } from "react";

import { agent } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

export function ChatComposer({
  sessionId,
  phase,
  onSent,
}: {
  sessionId: string;
  phase: StreamPhase;
  onSent: (text: string, sentAt: number) => void;
}) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);

  const disabled = phase === "connecting" || phase === "streaming" || sending;

  async function submit() {
    const t = text.trim();
    if (!t || disabled) return;
    const sentAt = Date.now();
    setSending(true);
    setErr(null);
    try {
      await agent.resume({ session_id: sessionId, observation: { kind: "UserReply", text: t } });
      setText("");
      ref.current?.focus();
      onSent(t, sentAt);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="bg-background rounded-md border">
      <Textarea
        ref={ref}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={disabled ? "Waiting for the agent to settle…" : "Type a message… (⌘↵ to send)"}
        rows={2}
        disabled={disabled}
        data-input="composer-text"
        className="resize-none border-0 focus-visible:ring-0"
      />
      {err && <div className="text-destructive border-t px-3 py-2 text-xs">× {err}</div>}
      <div className="flex items-center justify-between border-t px-3 py-2">
        <span className="text-muted-foreground text-xs">
          {disabled ? "Agent busy…" : "⌘↵ to send"}
        </span>
        <Button
          size="sm"
          onClick={submit}
          disabled={!text.trim() || disabled}
          data-action="send-reply"
        >
          {sending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Send className="h-4 w-4" />}
          Send
        </Button>
      </div>
    </div>
  );
}
