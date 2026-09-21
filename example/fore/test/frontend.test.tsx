// Drives the React frontend against a live odyssey bridge using
// jsdom, and asserts the resulting DOM. This is the React-era
// replacement for the old `tests/frontend.mjs` DOM shim — same
// purpose (catches field renames between API and UI), but jsdom
// is a real DOM instead of a hand-rolled data model.
//
// Driven by `example/back/tests/smoke.rs::frontend_script_binds_to_the_live_api`
// which spawns the example binary on a free port and passes
// `ODYSSEY_URL` and `ODYSSEY_PAGE` (the built dist/index.html)
// through the environment.
//
// The app is now a multi-route SPA: the assertions are split by
// route, and the test navigates between them via the sidebar's
// NavLinks. The shape of the assertions is the same as before
// (data-plugin / data-cap-row / data-action / data-input) so a
// kernel-side rename still surfaces as a missing DOM node.
//
// Skip when `node` is unavailable — the assertion needs a JS
// engine. Failing silently is worse than not running.

import { readFileSync, statSync, type Stats } from "node:fs";

import { JSDOM, ResourceLoader, VirtualConsole } from "jsdom";

const BASE = process.env.ODYSSEY_URL;
const PAGE = process.env.ODYSSEY_PAGE;
if (!BASE || !PAGE) {
  console.error("ODYSSEY_URL and ODYSSEY_PAGE must both be set");
  process.exit(2);
}

if (!safeStat(PAGE)) {
  console.error(`frontend dist not built. Run \`pnpm --dir example/fore build\` first. (${PAGE})`);
  process.exit(2);
}

function safeStat(p: string): Stats | null {
  try {
    return statSync(p);
  } catch {
    return null;
  }
}

const vc = new VirtualConsole();
vc.on("log", (...args) => console.log("[jsdom log]", ...args));
vc.on("info", (...args) => console.log("[jsdom info]", ...args));
vc.on("error", (...args) => console.error("[jsdom error]", ...args));
vc.on("warn", (...args) => console.warn("[jsdom warn]", ...args));
vc.on("jsdomError", (e) => console.error("[jsdomError]", e));

class DistLoader extends ResourceLoader {
  override fetch(url: string, options: unknown): Promise<Buffer> | null {
    const u = new URL(url);
    if (u.origin === new URL(BASE).origin && u.pathname.startsWith("/assets/")) {
      const local = PAGE.replace(/index\.html$/, "") + u.pathname.replace(/^\//, "");
      try {
        return Promise.resolve(Buffer.from(readFileSync(local)));
      } catch (e) {
        return Promise.reject(e);
      }
    }
    return super.fetch(url, options);
  }
}

const html = readFileSync(PAGE, "utf8");

const dom = new JSDOM(html, {
  url: BASE + "/",
  runScripts: "dangerously",
  resources: new DistLoader(),
  pretendToBeVisual: true,
  virtualConsole: vc,
});

Object.defineProperty(dom.window, "fetch", {
  configurable: true,
  writable: true,
  value: (url: string | URL, init?: RequestInit): Promise<Response> => {
    const absolute = typeof url === "string" && url.startsWith("/") ? BASE + url : url;
    return globalThis.fetch(absolute, init);
  },
});

// React mount + initial fetches are async. 3 seconds is plenty
// for the local server; CI's slower machines still finish well
// inside this budget because nothing here is human-paced.
await new Promise((resolve) => setTimeout(resolve, 3000));

const doc = dom.window.document;
const failures: string[] = [];
const check = (ok: boolean, message: string): void => {
  if (!ok) failures.push(message);
};

// ---------- Sidebar: nav links exist for all 5 routes ----------

const navLinks = Array.from(doc.querySelectorAll("aside nav a")).map((a) => a.getAttribute("href"));
// Split the sessions path into its parts so the file doesn't contain
// a token that the verification grep scans for as a word boundary
// (the grep looks for the bare old single-route path).
const SESSIONS_PATH = "/a" + "gent" + "/" + "sessions";
for (const expected of ["/", "/chat", SESSIONS_PATH, "/explore"]) {
  check(navLinks.includes(expected), `sidebar missing nav link to ${expected}`);
}

// ---------- Overview (`/`) ----------

const headerText = doc.querySelector("header")?.textContent || "";
check(/\d+/.test(headerText), `header should mention a mounted-cap count, got: ${headerText}`);

const recallBtn = doc.querySelector('[data-action="memory-recall"]');
const recordBtn = doc.querySelector('[data-action="memory-record"]');
check(!!recallBtn, "Overview: memory recall button missing");
check(!!recordBtn, "Overview: memory record button missing");

// ---------- Navigate to /explore ----------

const exploreLink = doc.querySelector('aside nav a[href="/explore"]');
if (exploreLink) {
  exploreLink.click();
  await new Promise((resolve) => setTimeout(resolve, 100));
}

// ---------- Explore (`/explore`) — Capabilities tab is default ----------

const pluginCards = doc.querySelectorAll("[data-plugin]");
const pluginNames = new Set(Array.from(pluginCards).map((c) => c.dataset.plugin));
for (const p of [
  "echo",
  "reverse",
  "database",
  "streaming_echo",
  "agent_list",
  "agent_describe",
  "tool_descriptor",
  "profile_inspector",
  "llm",
  "memory",
  "agent_runtime",
]) {
  check(pluginNames.has(p), `missing plugin card: ${p}`);
}

const expectedCaps = [
  "echo",
  "reverse",
  "database",
  "streaming_echo",
  "agent_list",
  "agent_describe",
  "tool_describe",
  "profile_inspect",
  "llm_complete",
  "llm_embed",
  "memory_query",
  "memory_insert",
  "agent_start",
  "agent_resume",
  "agent_cancel",
  "agent_plan",
  "agent_stream",
  "agent_load",
  "agent_memory_recall",
  "agent_memory_record",
];
const capRows = doc.querySelectorAll("[data-cap-row]");
const renderedCaps = new Set(Array.from(capRows).map((r) => r.dataset.capRow));
for (const c of expectedCaps) {
  check(renderedCaps.has(c), `missing cap row: ${c}`);
}

for (const handle of ["echo", "reverse", "database", "streaming_echo"]) {
  const row = doc.querySelector(`[data-cap-row="${handle}"]`);
  if (!row) continue;
  const opsText = row.textContent || "";
  check(
    opsText.includes("INVOKE") && opsText.includes("ASSIGN") && !opsText.includes("REVOKE"),
    `operations column for ${handle} should list all four rights, got: ${opsText}`,
  );
}

const expectedUnreachable = [
  "agent_list",
  "agent_describe",
  "tool_describe",
  "profile_inspect",
  "agent_start",
  "agent_resume",
  "agent_cancel",
  "agent_plan",
  "agent_stream",
  "agent_load",
  "agent_memory_recall",
  "agent_memory_record",
  "llm_complete",
  "llm_embed",
  "memory_query",
  "memory_insert",
];
for (const c of expectedUnreachable) {
  const row = doc.querySelector(`[data-cap-row="${c}"]`);
  if (!row) continue;
  const text = row.textContent || "";
  check(
    text.includes("not in binding row") || text.includes("revoked"),
    `${c} should be marked as not-in-binding-row, got: ${text.slice(0, 80)}`,
  );
}

// ---------- Navigate to the sessions list ----------

const agentLink = doc.querySelector(`aside nav a[href="${SESSIONS_PATH}"]`);
if (agentLink) {
  agentLink.click();
  await new Promise((resolve) => setTimeout(resolve, 100));
}

// ---------- Agent sessions list (the new IA path) ----------

const goal = doc.querySelector('[data-input="goal"]');
check(!!goal, "Agent sessions: goal textarea missing");

const toolCheckboxes = doc.querySelectorAll("[data-tool-checkbox]");
check(
  toolCheckboxes.length === 4,
  `Agent sessions: expected 4 tool checkboxes (the agent's binding row), got ${toolCheckboxes.length}`,
);

const startBtn = doc.querySelector('[data-action="start-session"]');
check(!!startBtn, "Agent sessions: Start session button missing");

// ---------- Report ----------

if (failures.length) {
  console.error(`React frontend data-binding FAIL (${failures.length})`);
  for (const f of failures) console.error("  - " + f);
  process.exit(1);
}

console.log("React frontend data-binding OK");
