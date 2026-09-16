// Router shell. Each route is a real page under `src/pages/`;
// the `AppShell` wraps them with sidebar + header so the
// layout chrome stays constant across navigation.
//
// Phase A: every page gets a placeholder while the real IA
// is built. The data hooks (`useCaps`, `useAgentSession`)
// already carry the state, so individual pages just pull
// from them rather than re-fetching.

import { Route, Routes } from "react-router-dom";
import { AppShell } from "./components/layout/AppShell";
import { Overview } from "./pages/Overview";
import { Caps } from "./pages/Caps";
import { CapDetail } from "./pages/CapDetail";
import { Agent } from "./pages/Agent";
import { AgentSession } from "./pages/AgentSession";
import { Invoke } from "./pages/Invoke";
import { Playground } from "./pages/Playground";
import { Checkpoints } from "./pages/Checkpoints";

export function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Overview />} />
        <Route path="caps" element={<Caps />} />
        <Route path="caps/:name" element={<CapDetail />} />
        <Route path="agent" element={<Agent />} />
        <Route path="agent/:sessionId" element={<AgentSession />} />
        <Route path="invoke" element={<Invoke />} />
        <Route path="playground" element={<Playground />} />
        <Route path="checkpoints" element={<Checkpoints />} />
        <Route path="*" element={<Overview />} />
      </Route>
    </Routes>
  );
}
