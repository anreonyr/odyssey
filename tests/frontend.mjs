// Loads examples/frontend/index.html's inline script into a minimal
// DOM shim and drives it against a live server, then asserts what the
// page would display.
//
// Why this exists: `node --check` proves the script parses, and curl
// proves the endpoints answer, but neither proves the render code
// reads the keys the API actually returns. Every field the page
// touches is checked against a real response here, and a rename on
// either side fails the test instead of silently rendering dashes.
//
// The shim is a data model, not a browser: it records textContent,
// className and the element tree. It cannot catch layout or CSS
// breakage — only data-binding breakage.
//
// Driven by tests/smoke.rs::frontend_script_binds_to_the_live_api,
// which spawns the example binary and passes the base URL and the
// page path through the environment.

import vm from 'node:vm';
import { readFileSync } from 'node:fs';

const BASE = process.env.ODYSSEY_URL;
const PAGE = process.env.ODYSSEY_PAGE;
if (!BASE || !PAGE) {
  console.error('ODYSSEY_URL and ODYSSEY_PAGE must both be set');
  process.exit(2);
}

class Element {
  constructor(tag) {
    this.tagName = tag;
    this.children = [];
    this.attrs = {};
    this.listeners = {};
    this._text = '';
    this._html = '';
    this.colSpan = 0;
    this.title = '';
  }
  set className(v) { this.attrs.class = v; }
  get className() { return this.attrs.class || ''; }
  set textContent(v) { this._text = String(v); this.children = []; }
  get textContent() { return this._text; }
  set innerHTML(v) { this._html = String(v); this._text = ''; this.children = []; }
  get innerHTML() { return this._html; }
  appendChild(c) { this.children.push(c); return c; }
  get value() { return this._value || ''; }
  set value(v) { this._value = v; }
  get onclick() { return this.listeners.click; }
  set onclick(fn) { this.listeners.click = fn; }
}

// Every id the page resolves at load time.
const ids = [
  'caps', 'caps-note', 'agent', 'agent-table', 'describe-handle', 'describe-out',
  'single-cap', 'single-input', 'single-out',
  'stream-cap', 'stream-input', 'stream-out',
];
const byId = Object.fromEntries(ids.map((id) => [id, new Element('div')]));

// The page fetches relative paths, which a browser resolves against the
// document origin; Node needs the base made explicit.
const shimFetch = (url, init) =>
  fetch(typeof url === 'string' && url.startsWith('/') ? BASE + url : url, init);

const sandbox = {
  document: {
    getElementById: (id) => byId[id] ?? null,
    createElement: (tag) => new Element(tag),
    createTextNode: (t) => { const e = new Element('#text'); e.textContent = t; return e; },
  },
  fetch: shimFetch,
  console,
  TextDecoder,
  setTimeout,
  clearTimeout,
};

const script = readFileSync(PAGE, 'utf8').match(/<script>([\s\S]*?)<\/script>/)[1];
if (!script) {
  console.error('no inline <script> found in the page');
  process.exit(2);
}

process.on('unhandledRejection', (e) => {
  console.error('unhandled rejection from the page script:', e);
  process.exit(1);
});

const context = vm.createContext(sandbox);
vm.runInContext(script, context, { filename: 'frontend.js' });

// boot() runs on load and is fire-and-forget in the page; let it settle.
await new Promise((resolve) => setTimeout(resolve, 1500));

const failures = [];
const check = (ok, message) => { if (!ok) failures.push(message); };
// A dash is written through innerHTML (no child nodes), so read both.
const flatten = (el) =>
  el ? el.textContent + el.innerHTML + el.children.map(flatten).join(' ') : '';
const strip = (s) => s.replace(/<[^>]*>/g, '');
const walk = (el) => [el, ...el.children.flatMap(walk)];

// --- the agent panel reads agent_list and agent_describe ---------------
const agentText = flatten(byId.agent);
check(!agentText.includes('agent unavailable'), `agent panel showed an error: ${agentText}`);
for (const handle of ['echo', 'reverse', 'database', 'streaming_echo']) {
  check(agentText.includes(handle), `agent panel is missing handle ${handle}`);
}
check(agentText.includes('✓'), 'live handles should be marked live');

const table = byId['agent-table'].children[0];
check(!!table, 'the describe table was not rendered');
if (table) {
  const rows = table.children;
  check(rows.length === 5, `expected 1 header + 4 handle rows, got ${rows.length}`);
  const header = rows[0].children.map((th) => th.textContent).join(',');
  check(header === 'handle,name,kind,operations,timeout', `table header mismatch: ${header}`);

  const kinds = rows.slice(1).map((tr) => tr.children[2].textContent);
  check(kinds.includes('stream'), `no stream row rendered: ${kinds.join(',')}`);
  check(
    kinds.filter((k) => k === 'sync').length === 3,
    `expected 3 sync rows, got: ${kinds.join(',')}`,
  );

  // `operations` is the whole point of routing through the agent: the
  // cspace cannot report rights, `AnyCapability::operations()` can.
  const ops = rows.slice(1).map((tr) => tr.children[3].children.map((s) => s.textContent).join('|'));
  check(
    ops.every((v) => v === 'READ|WRITE|EXECUTE|ADMIN'),
    `operations column did not render four rights: ${ops.join(' , ')}`,
  );

  const timeouts = rows.slice(1).map((tr) => tr.children[4].textContent);
  check(timeouts.every((v) => v === '5000ms'), `timeout column mismatch: ${timeouts.join(',')}`);

  const chip = rows[1]?.children[0]?.children[0];
  check(typeof chip?.onclick === 'function', 'row handle chips must be clickable');
}

// --- the capability list degrades to unreachable for agent itself ------
const capsText = flatten(byId.caps);
for (const name of ['echo', 'reverse', 'database', 'streaming_echo', 'agent_list', 'agent_describe']) {
  check(capsText.includes(name), `capability chips are missing ${name}`);
}

// The agents are mounted but appear in no binding row — not their own,
// and not each other's — so the page must report them as unreachable.
const capsNote = strip(flatten(byId['caps-note']));
check(
  capsNote.includes('agent unreachable'),
  `expected the two agent capabilities to be reported unreachable, got: "${capsNote}"`,
);
for (const name of ['agent_list', 'agent_describe']) {
  check(capsNote.includes(name), `the unreachable note should name ${name}`);
}
const dimmed = walk(byId.caps).filter((e) => e.className.includes('unreachable'));
check(dimmed.length === 2, `expected exactly 2 dimmed chips, got ${dimmed.length}`);

// --- a revoked row ----------------------------------------------------
// A revoked capability answers with only handle/live/capability/contract,
// so `kind` is absent exactly when the row is dead. The shim cannot make
// the live server revoke on demand, so drive describeRow directly: a dead
// stream cap must stay marked streaming and dash its absent fields.
const dead = context.describeRow('streaming_echo', true, {
  handle: 'streaming_echo', live: false,
  capability: 'streaming_echo', contract: 'streaming_echo',
});
const deadChip = dead.children[0].children[0];
check(
  deadChip.className.includes('streaming'),
  `a revoked stream cap must stay marked streaming, got: ${deadChip.className}`,
);
check(deadChip.className.includes('unreachable'), 'a revoked row chip should read as dead');
check(typeof deadChip.onclick === 'function', 'a revoked row chip should still be clickable');
const deadCells = dead.children.slice(1).map((td) => strip(flatten(td)).trim());
check(
  deadCells.every((v) => v === '—'),
  `a revoked row should dash its capability-level fields, got: ${deadCells.join(' | ')}`,
);
const deadSync = context.describeRow('echo', false, {
  handle: 'echo', live: false, capability: 'echo', contract: 'echo',
});
check(
  !deadSync.children[0].children[0].className.includes('streaming'),
  'a revoked sync cap must not be drawn as streaming',
);

// --- the describe form ------------------------------------------------
await context.describe('echo');
const described = byId['describe-out'].textContent;
check(described.includes('"live": true'), 'describe output is missing live:true');
check(described.includes('"operations"'), 'describe output is missing operations');
check(byId['describe-handle'].value === 'echo', 'the handle field was not synced');

// --- report -----------------------------------------------------------
if (failures.length) {
  console.error(`frontend data-binding FAIL (${failures.length})`);
  for (const f of failures) console.error('  - ' + f);
  process.exit(1);
}
console.log('frontend data-binding OK');
