// Agent page (`/agent`). Two columns:
//   - Left: session list (existing) + start form
//   - Right: contextual panel — empty state until a session is selected,
//            then it links to /agent/:sessionId
//
// The full timeline lives on the dedicated session route so it has room
// to breathe; this page is the launchpad.

import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { ArrowRight, Play, Loader2, X, Save, FolderOpen } from "lucide-react";
import { useCaps } from "../hooks/useCaps";
import { useAgentSession } from "../hooks/useAgentSession";
import type { SessionEntry } from "../hooks/useAgentSession";
import type { SessionStatus } from "../api/types";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { Textarea } from "../components/ui/textarea";
import { Badge } from "../components/ui/badge";
import { Separator } from "../components/ui/separator";
import { cn } from "../lib/utils";

export function Agent() {
  const { reachable } = useCaps();
  const sessionStore = useAgentSession();
  const navigate = useNavigate();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showLoad, setShowLoad] = useState(false);
  const [loadPath, setLoadPath] = useState("");

  const sessions = Array.from(sessionStore.sessions.values());
  const selected = selectedId ? sessionStore.sessions.get(selectedId) : null;
  const [ckptFor, setCkptFor] = useState<string | null>(null);
  const [ckptPath, setCkptPath] = useState("");

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-[360px,1fr]">
      {/* Left: sessions list + start form */}
      <div className="space-y-4">
        {/* Start form */}
        <Card>
          <CardHeader>
            <CardTitle>New session</CardTitle>
          </CardHeader>
          <CardContent>
            <StartForm
              reachable={Array.from(reachable.keys())}
              busy={sessionStore.busy}
              onStart={async (input) => {
                const id = await sessionStore.start(input);
                setSelectedId(id);
                navigate(`/agent/${id}`);
              }}
            />
          </CardContent>
        </Card>

        {/* Sessions list */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0">
            <CardTitle>Sessions ({sessions.length})</CardTitle>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowLoad((v) => !v)}
              data-action="toggle-load"
            >
              <FolderOpen className="h-3.5 w-3.5" />
              Load checkpoint
            </Button>
          </CardHeader>
          {showLoad && (
            <CardContent className="flex gap-2 pb-3">
              <Input
                placeholder="/path/to/checkpoint.json"
                value={loadPath}
                onChange={(e) => setLoadPath(e.target.value)}
                data-input="load-path"
              />
              <Button
                size="sm"
                variant="default"
                disabled={!loadPath || sessionStore.busy}
                onClick={() => {
                  sessionStore.load(loadPath);
                  setShowLoad(false);
                }}
                data-action="load-checkpoint"
              >
                load
              </Button>
            </CardContent>
          )}
          <CardContent className="p-0">
            {sessions.length === 0 ? (
              <p className="px-4 pb-4 text-xs text-muted-foreground">
                no sessions yet — start one above.
              </p>
            ) : (
              <ul>
                {sessions.map((s) => (
                  <li key={s.session_id}>
                    <button
                      type="button"
                      data-session-id={s.session_id}
                      onClick={() => setSelectedId(s.session_id)}
                      className={cn(
                        "flex w-full items-center gap-2 border-b border-border/50 px-4 py-2.5 text-left text-xs transition-colors last:border-b-0 hover:bg-accent/50",
                        selectedId === s.session_id && "bg-accent",
                      )}
                    >
                      <StatusPill status={s.status} />
                      <code className="font-mono text-[10px] text-muted-foreground">
                        {s.session_id.slice(0, 12)}…
                      </code>
                      <span className="flex-1 truncate">{s.goal}</span>
                      <span className="font-mono text-[10px] tabular-nums text-muted-foreground">
                        {s.history_len}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        {sessionStore.error && (
          <Card className="border-destructive/40">
            <CardContent className="p-4">
              <pre className="font-mono text-xs text-destructive">
                {sessionStore.error}
              </pre>
            </CardContent>
          </Card>
        )}
      </div>

      {/* Right: session detail */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <CardTitle>
            {selected ? `Session ${selected.session_id.slice(0, 12)}…` : "Pick a session"}
          </CardTitle>
          {selected && (
            <Button asChild size="sm" variant="outline">
              <Link to={`/agent/${selected.session_id}`}>
                Open timeline <ArrowRight className="h-3.5 w-3.5" />
              </Link>
            </Button>
          )}
        </CardHeader>
        <CardContent>
          {!selected ? (
            <p className="text-xs text-muted-foreground">
              select a session on the left to see its goal, tools, and history
              length — or jump straight into the timeline.
            </p>
          ) : (
            <SessionSummary
              session={selected}
              busy={sessionStore.busy}
              onCancel={() => sessionStore.cancel(selected.session_id)}
              onCancelCheckpoint={() => setCkptFor(selected.session_id)}
            />
          )}
        </CardContent>
      </Card>

      {/* Inline checkpoint dialog */}
      {ckptFor && (
        <div className="lg:col-span-2">
          <Card className="border-warning/40">
            <CardContent className="flex items-center gap-2 p-4">
              <Save className="h-3.5 w-3.5 text-warning" />
              <span className="text-xs">checkpoint path</span>
              <Input
                value={ckptPath}
                onChange={(e) => setCkptPath(e.target.value)}
                placeholder="/path/to/checkpoint.json"
                className="flex-1"
                data-input="checkpoint-path"
              />
              <Button
                size="sm"
                variant="success"
                disabled={!ckptPath || sessionStore.busy}
                onClick={() => {
                  sessionStore.cancel(ckptFor, ckptPath);
                  setCkptFor(null);
                  setCkptPath("");
                }}
                data-action="save-checkpoint"
              >
                save & cancel
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setCkptFor(null)}>
                ×
              </Button>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}

function StatusPill({ status }: { status: SessionStatus }) {
  const map: Record<SessionStatus, { variant: "success" | "warning" | "muted" | "destructive" | "default"; label: string }> = {
    Running: { variant: "success", label: "running" },
    AwaitingObservation: { variant: "warning", label: "awaiting" },
    Done: { variant: "muted", label: "done" },
    Failed: { variant: "destructive", label: "failed" },
    Cancelled: { variant: "destructive", label: "cancelled" },
  };
  const m = map[status];
  return <Badge variant={m.variant}>{m.label}</Badge>;
}

function StartForm({
  reachable,
  busy,
  onStart,
}: {
  reachable: string[];
  busy: boolean;
  onStart: (input: { goal: string; allowed_tools?: string[] }) => Promise<void>;
}) {
  const [goal, setGoal] = useState("");
  const [tools, setTools] = useState<Set<string>>(new Set(reachable));

  function toggle(t: string) {
    setTools((prev) => {
      const next = new Set(prev);
      if (next.has(t)) next.delete(t);
      else next.add(t);
      return next;
    });
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!goal.trim()) return;
        onStart({ goal, allowed_tools: Array.from(tools) });
        setGoal("");
      }}
      className="space-y-3"
    >
      <div>
        <label className="mb-1 block text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
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
        <label className="mb-1 block text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
          tools ({tools.size}/{reachable.length})
        </label>
        <div className="flex flex-wrap gap-1.5">
          {reachable.map((t) => (
            <label
              key={t}
              className={cn(
                "inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 font-mono text-[11px] transition-colors",
                tools.has(t)
                  ? "border-primary/50 bg-primary/10 text-foreground"
                  : "border-border bg-card text-muted-foreground hover:border-border/80",
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
        </div>
      </div>
      <Button
        type="submit"
        size="sm"
        variant="success"
        disabled={!goal.trim() || busy}
        data-action="start-session"
      >
        {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5" />}
        Start session
      </Button>
    </form>
  );
}

function SessionSummary({
  session,
  busy,
  onCancel,
  onCancelCheckpoint,
}: {
  session: SessionEntry;
  busy: boolean;
  onCancel: () => void;
  onCancelCheckpoint: () => void;
}) {
  return (
    <div className="space-y-3 text-xs">
      <Meta label="goal" value={session.goal} block />
      <Meta label="status" value={<StatusPill status={session.status} />} />
      <Meta
        label="tools"
        value={
          <div className="flex flex-wrap gap-1">
            {session.allowed_tools.map((t) => (
              <Badge key={t} variant="muted">
                {t}
              </Badge>
            ))}
          </div>
        }
      />
      <Meta label="history" value={`${session.history_len} steps`} />
      {session.checkpoint_path && <Meta label="checkpoint" value={session.checkpoint_path} />}
      <Separator />
      <div className="flex items-center gap-2">
        <Button size="sm" variant="destructive" disabled={busy || session.status === "Cancelled"} onClick={onCancel} data-action="cancel">
          <X className="h-3.5 w-3.5" /> cancel
        </Button>
        <Button size="sm" variant="outline" disabled={busy || session.status === "Cancelled"} onClick={onCancelCheckpoint} data-action="cancel-checkpoint">
          <Save className="h-3.5 w-3.5" /> cancel &amp; checkpoint
        </Button>
      </div>
    </div>
  );
}

function Meta({ label, value, block }: { label: string; value: React.ReactNode; block?: boolean }) {
  return (
    <div className={cn("flex gap-3", block ? "flex-col" : "items-baseline justify-between")}>
      <span className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <div className={block ? "" : "text-right"}>{value}</div>
    </div>
  );
}
