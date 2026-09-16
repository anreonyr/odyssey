// Checkpoint browser (`/checkpoints`). Lists the files the
// agent has written to disk; click one to open the session it
// belongs to (loads the file via agent_load).
//
// The Rust side tracks checkpoint paths from the agent's
// `cancel` cap; this page is just a viewer over
// `GET /api/checkpoints`.

import { Link, useNavigate } from "react-router-dom";
import { FolderOpen, Loader2, AlertCircle, ArrowRight } from "lucide-react";
import { useCheckpoints } from "../hooks/useCheckpoints";
import { agent } from "../api/client";
import { useAgentSession } from "../hooks/useAgentSession";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Badge } from "../components/ui/badge";

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

function formatTime(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toISOString().slice(0, 19).replace("T", " ");
}

export function Checkpoints() {
  const { checkpoints, loading, error } = useCheckpoints();
  const sessionStore = useAgentSession();
  const navigate = useNavigate();

  async function load(path: string) {
    try {
      const r = await agent.load({ path });
      sessionStore.cancel; // touch to keep the linter happy about hooks ordering
      navigate(`/agent/${r.session_id}`);
    } catch (e) {
      // Surface as session-store error so the user sees it on the
      // destination route.
      // eslint-disable-next-line no-alert
      window.alert(`failed to load: ${(e as Error).message}`);
    }
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-xs">Saved checkpoints ({checkpoints.length})</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          {loading ? (
            <div className="flex items-center gap-2 p-4 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              loading…
            </div>
          ) : error ? (
            <div className="flex items-center gap-2 p-4 text-xs text-destructive">
              <AlertCircle className="h-3.5 w-3.5" />
              {error}
            </div>
          ) : checkpoints.length === 0 ? (
            <p className="p-4 text-xs text-muted-foreground">
              no checkpoints yet — cancel a session with a path to save one.
            </p>
          ) : (
            <ul>
              {checkpoints.map((c) => (
                <li
                  key={c.path}
                  className="flex items-center gap-3 border-b border-border/50 px-4 py-2.5 last:border-b-0 hover:bg-accent/50"
                >
                  <FolderOpen className="h-3.5 w-3.5 text-muted-foreground" />
                  <div className="min-w-0 flex-1">
                    <code className="block truncate font-mono text-xs">{c.path}</code>
                    <div className="mt-0.5 flex items-center gap-2 text-[10px] text-muted-foreground">
                      <span className="font-mono">{formatTime(c.saved_at)}</span>
                      <Badge variant="muted">{formatBytes(c.size)}</Badge>
                      <Link
                        to={`/agent/${c.session_id}`}
                        className="font-mono hover:text-foreground"
                      >
                        session {c.session_id.slice(0, 12)}…
                      </Link>
                    </div>
                  </div>
                  <Button size="sm" variant="outline" onClick={() => load(c.path)}>
                    load <ArrowRight className="h-3.5 w-3.5" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
