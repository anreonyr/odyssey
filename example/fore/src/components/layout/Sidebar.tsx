// Sidebar — fixed-width navigation rail. Active link gets a
// background-accent fill; inactive links are muted.
//
// Counts (caps, sessions) come from the same hooks the pages
// use, so the sidebar's badges always reflect the same source
// of truth as the page content.
//
// Receives arbitrary HTMLAttributes so AppShell can pass
// `data-stagger-item` + style for the entrance choreography
// (DESIGN.md "动作"). Props.className is intentionally ignored:
// the sidebar's chrome is fixed, callers shouldn't override it.

import type { HTMLAttributes } from "react";

import { Bot, LayoutDashboard, MessageSquare, Sparkles } from "lucide-react";
import { NavLink } from "react-router-dom";

import { useAgentSession } from "@/hooks/useAgentSession";
import { useCaps } from "@/hooks/useCaps";
import { cn } from "@/lib/utils";

interface NavItem {
  to: string;
  label: string;
  // `className?: string | undefined` matches lucide-react's
  // LucideIcon signature under `exactOptionalPropertyTypes: true`.
  icon: React.ComponentType<{ className?: string | undefined }>;
  badge?: () => number | null;
}

export function Sidebar(props: HTMLAttributes<HTMLElement>) {
  const { reachable } = useCaps();
  const { sessions } = useAgentSession();

  const items: NavItem[] = [
    { to: "/", label: "Overview", icon: LayoutDashboard },
    { to: "/chat", label: "Chat", icon: MessageSquare, badge: () => sessions.size || null },
    {
      to: "/agent/sessions",
      label: "Agent sessions",
      icon: Bot,
      badge: () => sessions.size || null,
    },
    { to: "/explore", label: "Explore", icon: Sparkles, badge: () => reachable.size || null },
  ];

  return (
    <aside {...props} className="bg-background flex h-screen w-60 shrink-0 flex-col border-r">
      <div className="flex h-12 items-center gap-2 border-b px-4">
        <span className="text-sm font-semibold tracking-tight">odyssey</span>
        <span className="text-muted-foreground ml-auto text-xs">v0.1</span>
      </div>

      <nav className="flex-1 space-y-0.5 overflow-y-auto p-2">
        {items.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.to === "/"}
            className={({ isActive }) =>
              cn(
                "flex h-8 items-center gap-2.5 rounded-md px-3 text-sm transition-colors",
                isActive
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-accent-foreground",
              )
            }
          >
            <item.icon className="h-4 w-4 shrink-0" />
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            {item.badge && item.badge() != null && (
              <span className="text-muted-foreground shrink-0 text-xs tabular-nums">
                {item.badge()}
              </span>
            )}
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
