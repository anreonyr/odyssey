// Chat launcher (`/chat`). Starts a session and navigates to
// its stream view at `/chat/:id`. The companion
// `/agent/sessions/:id` page is the step-by-step controller;
// this page is the chat surface.
//
// Minimal launcher: the start-session form (goal + tools +
// start or load) and a list of recent sessions if any.

import { ArrowRight, FolderOpen, Loader2, Play } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { useAgentSession } from "@/hooks/useAgentSession";
import { useCaps } from "@/hooks/useCaps";

export function Chat() {
  return (
    <div className="space-y-6">
      <StartPanel />
      <RecentSessions />
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
    <Card>
      <CardHeader>
        <CardTitle>Start a session</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div>
          <label className="mb-2 block text-sm font-medium">Goal</label>
          <Textarea
            rows={3}
            placeholder="What should the agent do?"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            data-input="goal"
          />
        </div>

        <div>
          <label className="mb-2 block text-sm font-medium">
            Tools ({tools.size}/{toolList.length})
          </label>
          <div className="flex flex-wrap gap-2">
            {toolList.map((t) => (
              <label
                key={t}
                className={
                  tools.has(t)
                    ? "border-primary/40 bg-primary/10 inline-flex cursor-pointer items-center gap-2 rounded-md border px-2 py-1 font-mono text-sm"
                    : "bg-card text-muted-foreground hover:border-foreground/40 inline-flex cursor-pointer items-center gap-2 rounded-md border px-2 py-1 font-mono text-sm"
                }
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
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Button
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
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Play className="h-4 w-4" />
            )}
            Start & stream
          </Button>
          <Button variant="outline" onClick={() => setShowLoad((v) => !v)}>
            <FolderOpen className="h-4 w-4" />
            Load checkpoint
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
              disabled={!loadPath || sessionStore.busy}
              onClick={async () => {
                const id = await sessionStore.load(loadPath);
                setShowLoad(false);
                setLoadPath("");
                navigate(`/chat/${id}`);
              }}
              data-action="load-checkpoint"
            >
              Load
            </Button>
          </div>
        )}
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
      <h2 className="text-muted-foreground text-sm font-medium">Recent</h2>
      <ul className="divide-y rounded-md border">
        {sessions.map((s) => (
          <li key={s.session_id}>
            <Link
              to={`/chat/${s.session_id}`}
              data-session-id={s.session_id}
              className="hover:bg-accent/50 flex items-center gap-3 px-4 py-3 text-sm transition-colors"
            >
              <Badge variant={statusVariant(s.status)}>{s.status.toLowerCase()}</Badge>
              <code className="text-muted-foreground font-mono text-xs">
                {s.session_id.slice(0, 12)}…
              </code>
              <span className="flex-1 truncate">{s.goal}</span>
              <span className="text-muted-foreground font-mono text-xs tabular-nums">
                {s.history_len}
              </span>
              <ArrowRight className="text-muted-foreground h-4 w-4" />
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
