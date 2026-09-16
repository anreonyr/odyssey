// Chat launcher (`/chat`). Streams a real-time conversation
// with the agent — start a session and navigate to its stream
// view. The companion `/agent` page is the step-by-step
// controller; this page is the chat surface.
//
// Typography carries the page identity: oversized mono "A G E N T"
// header, ASCII rules, mono everywhere. The rest of the chrome
// is intentionally quiet so the header does the talking.

import { ArrowRight, FolderOpen, Loader2, Play } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";

import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { Textarea } from "../components/ui/textarea";
import { useAgentSession } from "../hooks/useAgentSession";
import { useCaps } from "../hooks/useCaps";
import { cn } from "../lib/utils";

export function Chat() {
  return (
    <div className="space-y-6">
      <Header />
      <StartPanel />
      <RecentSessions />
    </div>
  );
}

function Header() {
  return (
    <div className="space-y-3">
      <div className="flex items-baseline gap-4">
        <h1 className="text-foreground font-mono text-6xl font-bold leading-none tracking-tight md:text-8xl">
          A G E N T
        </h1>
        <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
          ── stream chat
        </span>
      </div>
      <pre className="text-muted-foreground font-mono text-[10px] leading-tight">
        {`──────────────────────────────────────────────────────────────────────────────
  START A SESSION · WATCH THE AGENT THINK TOKEN-BY-TOKEN · INTERJECT WITH
  USER REPLIES · EVERY EVENT FROM agent_stream LANDS HERE AS IT FIRES.
──────────────────────────────────────────────────────────────────────────────`}
      </pre>
    </div>
  );
}

function StartPanel() {
  const { reachable } = useCaps();
  const sessionStore = useAgentSession();
  const navigate = useNavigate();
  const [goal, setGoal] = useState("");
  const [tools, setTools] = useState<Set<string>>(new Set(reachable.keys()));
  const [loadPath, setLoadPath] = useState("");
  const [showLoad, setShowLoad] = useState(false);

  const toolList = Array.from(reachable.keys());

  function toggle(t: string) {
    setTools((prev) => {
      const next = new Set(prev);
      if (next.has(t)) next.delete(t);
      else next.add(t);
      return next;
    });
  }

  return (
    <Card className="border-2">
      <CardContent className="p-0">
        <div className="border-border bg-card/60 border-b-2 px-4 py-2">
          <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
            ╔ NEW SESSION
          </span>
        </div>

        <div className="grid grid-cols-1 gap-0 lg:grid-cols-[1fr,auto]">
          <div className="space-y-4 p-4">
            <div>
              <label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-widest">
                ── goal
              </label>
              <Textarea
                rows={3}
                placeholder="what should the agent do?"
                value={goal}
                onChange={(e) => setGoal(e.target.value)}
                data-input="goal"
              />
            </div>

            <div>
              <label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-widest">
                ── tools ({tools.size}/{toolList.length})
              </label>
              <div className="flex flex-wrap gap-2">
                {toolList.map((t) => (
                  <label
                    key={t}
                    className={cn(
                      "inline-flex cursor-pointer items-center gap-2 border-2 px-2 py-1 font-mono text-[11px] transition-colors",
                      tools.has(t)
                        ? "border-primary/50 bg-primary/10 text-foreground"
                        : "border-border bg-card text-muted-foreground hover:border-foreground/40",
                    )}
                  >
                    <input
                      type="checkbox"
                      className="sr-only"
                      checked={tools.has(t)}
                      onChange={() => toggle(t)}
                      data-tool-checkbox={t}
                    />
                    {t}
                  </label>
                ))}
                {toolList.length === 0 && (
                  <span className="text-muted-foreground font-mono text-[10px]">
                    no reachable tools — agent needs at least one capability bound.
                  </span>
                )}
              </div>
            </div>

            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="success"
                disabled={!goal.trim() || sessionStore.busy || toolList.length === 0}
                data-action="start-session"
                onClick={async () => {
                  const id = await sessionStore.start({
                    goal,
                    allowed_tools: Array.from(tools),
                  });
                  setGoal("");
                  navigate(`/chat/${id}`);
                }}
              >
                {sessionStore.busy ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Play className="h-3.5 w-3.5" />
                )}
                start &amp; stream
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setShowLoad((v) => !v)}
                data-action="toggle-load"
              >
                <FolderOpen className="h-3.5 w-3.5" />
                load checkpoint
              </Button>
            </div>

            {showLoad && (
              <div className="flex items-center gap-2">
                <Input
                  placeholder="/path/to/checkpoint.json"
                  value={loadPath}
                  onChange={(e) => setLoadPath(e.target.value)}
                  data-input="load-path"
                />
                <Button
                  size="sm"
                  disabled={!loadPath || sessionStore.busy}
                  onClick={async () => {
                    const id = await sessionStore.load(loadPath);
                    setShowLoad(false);
                    setLoadPath("");
                    navigate(`/chat/${id}`);
                  }}
                  data-action="load-checkpoint"
                >
                  load
                </Button>
              </div>
            )}
          </div>

          <div className="border-border bg-card/30 border-l-2 p-4 lg:w-72">
            <div className="text-muted-foreground mb-3 font-mono text-[10px] uppercase tracking-widest">
              ── stream events
            </div>
            <ul className="space-y-1.5 font-mono text-[11px]">
              <li>
                <span className="text-primary">≡</span> llm_reply_text / llm_delta
              </li>
              <li>
                <span className="text-warning">◇</span> tool_call
              </li>
              <li>
                <span className="text-muted-foreground">→</span> tool_result
              </li>
              <li>
                <span className="text-success">■</span> final (Goal / MaxSteps / ToolFailure)
              </li>
              <li>
                <span className="text-destructive">×</span> error
              </li>
            </ul>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function RecentSessions() {
  const sessionStore = useAgentSession();
  const sessions = Array.from(sessionStore.sessions.values()).slice(-5).reverse();

  if (sessions.length === 0) return null;

  return (
    <Card className="border-2">
      <CardContent className="p-0">
        <div className="border-border bg-card/60 border-b-2 px-4 py-2">
          <span className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
            ╔ RECENT SESSIONS
          </span>
        </div>
        <ul className="divide-border/50 divide-y">
          {sessions.map((s) => (
            <li key={s.session_id}>
              <Link
                to={`/chat/${s.session_id}`}
                data-session-id={s.session_id}
                className="hover:bg-accent/50 flex items-center gap-3 px-4 py-3 text-xs transition-colors"
              >
                <Badge variant={statusVariant(s.status)}>{s.status.toLowerCase()}</Badge>
                <code className="text-muted-foreground font-mono text-[10px]">
                  {s.session_id.slice(0, 16)}…
                </code>
                <span className="flex-1 truncate">{s.goal}</span>
                <span className="text-muted-foreground font-mono text-[10px] tabular-nums">
                  {s.history_len}
                </span>
                <ArrowRight className="text-muted-foreground h-3.5 w-3.5" />
              </Link>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function statusVariant(status: string): "success" | "warning" | "muted" | "destructive" {
  switch (status) {
    case "Running":
      return "success";
    case "AwaitingObservation":
      return "warning";
    case "Done":
      return "muted";
    case "Failed":
    case "Cancelled":
      return "destructive";
    default:
      return "muted";
  }
}
