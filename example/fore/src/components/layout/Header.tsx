// Header — top bar inside the main column. Replaces the
// single-row summary from the old App.tsx, but now the
// summary lives here (not the page), so navigation doesn't
// reset it and it stays visible while you scroll a long
// session timeline.
//
// The current route's page title comes from the URL; we
// derive a friendly name from `useLocation`.
//
// Receives arbitrary HTMLAttributes so AppShell can pass
// `data-stagger-item` + style for the entrance choreography
// (DESIGN.md "动作"). Props.className is intentionally ignored:
// the header's chrome is fixed.

import type { HTMLAttributes } from "react";

import { useLocation } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { useCaps } from "@/hooks/useCaps";

const TITLES: Array<[RegExp, string]> = [
  [/^\/$/, "Overview"],
  [/^\/chat\/[^/]+$/, "Chat"],
  [/^\/chat$/, "Chat"],
  [/^\/agent\/sessions\/[^/]+$/, "Session"],
  [/^\/agent\/sessions$/, "Agent sessions"],
  [/^\/explore/, "Explore"],
];

function titleFor(pathname: string): string {
  for (const [re, title] of TITLES) {
    if (re.test(pathname)) return title;
  }
  return "odyssey";
}

export function Header(props: HTMLAttributes<HTMLElement>) {
  const { pathname } = useLocation();
  const { caps, reachable, ready, agentError } = useCaps();
  const title = titleFor(pathname);

  return (
    <header
      {...props}
      className="bg-background flex h-12 shrink-0 items-center gap-3 border-b px-8"
    >
      <h1 className="text-sm font-semibold tracking-tight">{title}</h1>

      <div className="ml-auto flex items-center gap-2">
        <Badge variant="muted">
          <span className="text-muted-foreground">caps</span>
          <span className="font-mono tabular-nums">{caps.length}</span>
        </Badge>
        <Badge variant="muted">
          <span className="text-muted-foreground">reachable</span>
          <span className="font-mono tabular-nums">{reachable.size}</span>
        </Badge>
        <AgentBadge ready={ready} error={agentError} />
      </div>
    </header>
  );
}

function AgentBadge({ ready, error }: { ready: boolean; error: string | null }) {
  if (error) return <Badge variant="destructive">agent · down</Badge>;
  if (!ready) return <Badge variant="muted">agent · …</Badge>;
  return <Badge variant="success">agent · up</Badge>;
}
