// Router shell. Each route is a real page under `src/pages/`;
// the `AppShell` wraps them with sidebar + header so the
// layout chrome stays constant across navigation.
//
// Phase A: every page gets a placeholder while the real IA
// is built. The data hooks (`useCaps`, `useAgentSession`)
// already carry the state, so individual pages just pull
// from them rather than re-fetching.

import { Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/layout/AppShell";
import { AgentSession } from "@/pages/AgentSession";
import { AgentSessions } from "@/pages/AgentSessions";
import { Chat } from "@/pages/Chat";
import { ChatSession } from "@/pages/ChatSession";
import { Explore } from "@/pages/Explore";
import { Overview } from "@/pages/Overview";

export function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Overview />} />
        <Route path="chat" element={<Chat />} />
        <Route path="chat/:sessionId" element={<ChatSession />} />
        <Route path="agent/sessions" element={<AgentSessions />} />
        <Route path="agent/sessions/:sessionId" element={<AgentSession />} />
        <Route path="explore" element={<Explore />} />
        <Route path="*" element={<Overview />} />
      </Route>
    </Routes>
  );
}
