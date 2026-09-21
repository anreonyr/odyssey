// AppShell — the layout chrome that wraps every route:
//   ┌─ Sidebar ─┬─ Header (caps + agent status) ──────────┐
//   │           ├──────────────────────────────────────────┤
//   │           │  <Outlet />                              │
//   │           │                                          │
//   └───────────┴──────────────────────────────────────────┘
//
// Uses an `<Outlet />` from react-router so each route page
// renders into the right pane without re-mounting the sidebar
// (sidebar stays mounted → its state survives navigation).
//
// Entrance choreography (DESIGN.md "动作"): the root div is
// `data-stagger`; Sidebar, Header, and <main> are
// `data-stagger-item` with --stagger-index 1/2/3 so they
// resolve in sequence (rail → top bar → content). Total
// entrance ≤ 500ms (sidebar 110–390ms, header 160–440ms,
// main 210–490ms). Honors prefers-reduced-motion via the
// CSS rules in src/index.css.

import type { CSSProperties } from "react";

import { Outlet } from "react-router-dom";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionProvider } from "@/hooks/useAgentSession";

import { Header } from "./Header";
import { Sidebar } from "./Sidebar";

export function AppShell() {
  return (
    <TooltipProvider delayDuration={150}>
      <SessionProvider>
        <div
          className="bg-background text-foreground flex h-screen w-full overflow-hidden"
          data-stagger
        >
          <Sidebar data-stagger-item="" style={{ "--stagger-index": 1 } as CSSProperties} />
          <div className="flex flex-1 flex-col overflow-hidden">
            <Header data-stagger-item="" style={{ "--stagger-index": 2 } as CSSProperties} />
            <main
              className="flex-1 overflow-auto"
              data-stagger-item=""
              style={{ "--stagger-index": 3 } as CSSProperties}
            >
              <div className="mx-auto h-full w-full max-w-6xl px-8 py-6">
                <Outlet />
              </div>
            </main>
          </div>
        </div>
      </SessionProvider>
    </TooltipProvider>
  );
}
