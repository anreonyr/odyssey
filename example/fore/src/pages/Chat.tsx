// Chat launcher (`/chat`). Starts a session and navigates to
// its stream view at `/chat/:id`. The companion `/agent` page
// is the step-by-step controller; this page is the chat surface.
//
// Minimal launcher: a one-line tagline (Header.tsx already owns
// the page title), the start-session form (goal + tools + start
// or load), and a list of recent sessions if any. No big hero,
// no ASCII decoration — the grain texture provides atmosphere
// (see DESIGN.md "背景"). Same surgical treatment as
// ChatSession.tsx — drop the ╔ LABEL chrome, drop the "──"
// prefix on labels, keep functional UI intact.

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
      <Tagline />
      <StartPanel />
      <RecentSessions />
    </div>
  );
}

function Tagline() {
  return (
    <p className="text-muted-foreground font-mono text-xs leading-relaxed">
      start a session — watch the agent think token-by-token, interject with user replies.
    </p>
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
      <CardContent className="p-4">
        <div className="space-y-4">
          <div>
            <label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-widest">
              goal
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
              tools ({tools.size}/{toolList.length})
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
      </CardContent>
    </Card>
  );
}

function RecentSessions() {
  const sessionStore = useAgentSession();
  const sessions = Array.from(sessionStore.sessions.values()).slice(-5).reverse();

  if (sessions.length === 0) return null;

  return (
    <div className="space-y-2">
      <div className="text-muted-foreground font-mono text-[10px] uppercase tracking-widest">
        recent
      </div>
      <ul className="border-border divide-border/50 divide-y border-2">
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
    </div>
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
