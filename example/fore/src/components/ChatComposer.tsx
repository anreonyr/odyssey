// Mono composer for sending UserReply observations into a
// running session. Submitting triggers `agent_resume` with
// `{ kind: "UserReply", text }`; the kernel processes the
// observation and emits the resulting steps through the same
// `agent_stream` broadcast this page is already subscribed to.
//
// Disabled when the stream is connecting or done — the user
// can't interject while the session is finishing.

import type { StreamPhase } from "../hooks/useAgentStream";

import { Loader2, Send, X } from "lucide-react";
import { useRef, useState } from "react";

import { agent } from "../api/client";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";

export function ChatComposer({
  sessionId,
  phase,
  onSent,
  onCancel,
}: {
  sessionId: string;
  phase: StreamPhase;
  onSent: () => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);

  const disabled = phase === "connecting" || phase === "streaming" || sending;

  async function submit() {
    const t = text.trim();
    if (!t || disabled) return;
    setSending(true);
    setErr(null);
    try {
      await agent.resume({ session_id: sessionId, observation: { kind: "UserReply", text: t } });
      setText("");
      ref.current?.focus();
      onSent();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="border-border bg-card border-2">
      <div className="border-border bg-card/60 flex items-center justify-between border-b-2 px-3 py-1.5">
        <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
          ╔ COMPOSER
        </span>
        <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
          ╝
        </span>
      </div>

      <div className="flex">
        <div className="border-border text-primary bg-card/40 flex w-32 shrink-0 flex-col items-start gap-1 border-r-2 p-3 font-mono text-[10px] uppercase tracking-widest">
          <span className="font-bold">{">"} USER</span>
          <span className="text-muted-foreground normal-case tracking-tight">
            user reply observation
          </span>
        </div>

        <div className="flex-1 p-3">
          <textarea
            ref={ref}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                e.preventDefault();
                submit();
              }
            }}
            placeholder={
              disabled
                ? "stream in flight — wait for the agent to settle"
                : "type a user reply observation (⌘↵ to send)"
            }
            rows={3}
            disabled={disabled}
            data-input="composer-text"
            className={cn(
              "border-border bg-background text-foreground w-full resize-y border-2 p-2 font-mono text-xs leading-relaxed",
              "placeholder:text-muted-foreground focus:border-primary focus:outline-none",
              "disabled:opacity-50",
            )}
          />

          {err && <pre className="text-destructive mt-2 font-mono text-[10px]">× {err}</pre>}

          <div className="mt-2 flex items-center justify-end gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={onCancel}
              disabled={sending}
              data-action="cancel-session"
            >
              <X className="h-3.5 w-3.5" /> cancel session
            </Button>
            <Button
              size="sm"
              variant="success"
              onClick={submit}
              disabled={!text.trim() || disabled}
              data-action="send-reply"
            >
              {sending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Send className="h-3.5 w-3.5" />
              )}
              send
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
