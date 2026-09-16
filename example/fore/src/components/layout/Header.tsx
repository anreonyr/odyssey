// Header — top bar inside the main column. Replaces the
// single-row summary from the old App.tsx, but now the
// summary lives here (not the page), so navigation doesn't
// reset it and it stays visible while you scroll a long
// session timeline.
//
// The current route's page title comes from the URL; we
// derive a friendly name from `useLocation`.

import { useLocation } from "react-router-dom";

import { useCaps } from "../../hooks/useCaps";
import { Badge } from "../ui/badge";

const TITLES: Array<[RegExp, string]> = [
  [/^\/$/, "Overview"],
  [/^\/caps\/[^/]+$/, "Capability"],
  [/^\/caps$/, "Capabilities"],
  [/^\/agent\/[^/]+$/, "Session"],
  [/^\/agent$/, "Agent"],
  [/^\/invoke$/, "Invoke"],
  [/^\/playground$/, "Playground"],
  [/^\/checkpoints$/, "Checkpoints"],
];

function titleFor(pathname: string): string {
  for (const [re, title] of TITLES) {
    if (re.test(pathname)) return title;
  }
  return "odyssey";
}

export function Header() {
  const { pathname } = useLocation();
  const { caps, reachable, ready, agentError } = useCaps();
  const title = titleFor(pathname);

  return (
    <header className="border-border bg-card/40 flex h-12 shrink-0 items-center gap-3 border-b px-8">
      <h1 className="text-sm font-semibold tracking-tight">{title}</h1>
      <span className="text-muted-foreground text-xs">/ odyssey agent frontend</span>

      <div className="ml-auto flex items-center gap-2">
        <Badge variant="muted">
          <span className="text-muted-foreground">caps</span>
          <span className="text-foreground tabular-nums">{caps.length}</span>
        </Badge>
        <Badge variant="muted">
          <span className="text-muted-foreground">reachable</span>
          <span className="text-foreground tabular-nums">{reachable.size}</span>
        </Badge>
        <AgentBadge ready={ready} error={agentError} />
      </div>
    </header>
  );
}

function AgentBadge({ ready, error }: { ready: boolean; error: string | null }) {
  if (error) {
    return (
      <Badge variant="destructive" title={error}>
        agent · down
      </Badge>
    );
  }
  if (!ready) return <Badge variant="muted">agent · …</Badge>;
  return <Badge variant="success">agent · up</Badge>;
}
