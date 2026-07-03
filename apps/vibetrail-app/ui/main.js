// VibeTrail frontend: thin presentation over the Tauri IPC commands, which
// all delegate to vibetrail-core. No business logic here.
"use strict";

const invoke = window.__TAURI__.core.invoke;

const el = {
  projects: document.getElementById("project-list"),
  sessions: document.getElementById("session-list"),
  results: document.getElementById("search-results"),
  search: document.getElementById("search-input"),
  timeline: document.getElementById("timeline"),
  title: document.getElementById("detail-title"),
  resume: document.getElementById("resume-btn"),
  toast: document.getElementById("toast"),
  terminal: document.getElementById("terminal-select"),
  language: document.getElementById("language-select"),
  settingsBtn: document.getElementById("settings-btn"),
  settingsOverlay: document.getElementById("settings-overlay"),
  settingsClose: document.getElementById("settings-close"),
  settingsProviders: document.getElementById("settings-providers"),
  settingsHidden: document.getElementById("settings-hidden"),
  configPath: document.getElementById("config-path"),
  configReveal: document.getElementById("config-reveal"),
};

const state = {
  selectedProject: null,
  selectedSession: null,
  scrollTarget: null,
  projects: [],
  showHidden: false,
  // Mirror of the persisted AppConfig; always saved whole so one setting
  // never clobbers another.
  config: { terminal: "terminal", hiddenProjects: [], providers: {} },
};

// ---- helpers ---------------------------------------------------------------

function text(tag, className, content) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (content !== undefined) node.textContent = content;
  return node;
}

function relativeTime(iso) {
  const seconds = (Date.now() - new Date(iso).getTime()) / 1000;
  if (seconds < 60) return t("time.justNow");
  if (seconds < 3600) return t("time.minutesAgo", { n: Math.floor(seconds / 60) });
  if (seconds < 86400) return t("time.hoursAgo", { n: Math.floor(seconds / 3600) });
  if (seconds < 86400 * 30) return t("time.daysAgo", { n: Math.floor(seconds / 86400) });
  return new Date(iso).toISOString().slice(0, 10);
}

function compact(n) {
  if (!n) return "0";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "k";
  return String(n);
}

async function copySessionId(nativeId) {
  try {
    await invoke("copy_to_clipboard", { text: nativeId });
    toast(t("toast.idCopied", { id: nativeId }), true);
  } catch (error) {
    toast(String(error));
  }
}

function idChip(nativeId, full = false) {
  const chip = text("span", "id-chip", full ? nativeId : nativeId.slice(0, 8));
  chip.title = `${nativeId}\n${t("chip.copyTitle")}`;
  chip.addEventListener("click", (event) => {
    event.stopPropagation();
    copySessionId(nativeId);
  });
  return chip;
}

const AGENT_META = {
  "claude-code": { abbr: "CC", name: "Claude Code" },
  "codex": { abbr: "CX", name: "Codex" },
  "antigravity": { abbr: "AG", name: "Antigravity (experimental)" },
  "cursor": { abbr: "CU", name: "Cursor (experimental)" },
  "qoder": { abbr: "QD", name: "Qoder" },
};

// Neutral lettermark badges — vendor logos are off-limits (trademark
// hygiene); hover reveals the full agent name.
function agentBadge(providerId) {
  const meta = AGENT_META[providerId] || {
    abbr: providerId.slice(0, 2).toUpperCase(),
    name: providerId,
  };
  const badge = text("span", `agent-badge agent-${providerId}`, meta.abbr);
  badge.title = meta.name;
  return badge;
}

function shortPath(path) {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

let toastTimer;
function toast(message, info = false) {
  el.toast.textContent = message;
  el.toast.classList.toggle("info", info);
  el.toast.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.toast.classList.add("hidden"), 6000);
}

async function call(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    toast(String(error));
    throw error;
  }
}

// ---- projects (F1) ----------------------------------------------------------

async function loadProjects() {
  // First load shows a placeholder; refreshes keep the current list on
  // screen until fresh data arrives (no blank flash).
  if (state.projects.length === 0) {
    el.projects.replaceChildren(text("li", "notice", t("loading.projects")));
  }
  state.projects = await call("list_projects");
  renderProjects();
  warmSessionCache(); // fire-and-forget: makes first clicks instant
}

// Background warm-up: one whole-store discovery, grouped per project into
// the SWR cache. Clicks landing before it finishes just take the live path.
let warmGeneration = 0;
async function warmSessionCache() {
  const generation = ++warmGeneration;
  try {
    const all = await invoke("list_all_handles");
    if (generation !== warmGeneration) return;
    const grouped = new Map();
    for (const handle of all) {
      if (!grouped.has(handle.projectPath)) grouped.set(handle.projectPath, []);
      grouped.get(handle.projectPath).push(handle);
    }
    for (const [path, handles] of grouped) {
      handles.sort((a, b) => (a.mtime < b.mtime ? 1 : -1)); // newest first
      handleCache.set(path, handles);
    }
  } catch (_) { /* best-effort */ }
}

function projectRow(project, hidden) {
  const li = document.createElement("li");
  li.dataset.path = project.realPath;
  if (hidden) li.classList.add("hidden-project");
  if (project.realPath === state.selectedProject) li.classList.add("selected");
  const name = text("div", "name" + (project.exists ? "" : " orphaned"), shortPath(project.realPath));
  if (!project.exists) name.append(text("span", "", "⚠"));
  const toggle = text("button", "hide-btn", hidden ? "↩" : "⊘");
  toggle.title = hidden ? t("project.unhide") : t("project.hide");
  toggle.addEventListener("click", (event) => {
    event.stopPropagation();
    if (hidden) unhideProject(project);
    else hideProject(project);
  });
  name.append(toggle);
  li.append(name);
  const meta = text("div", "meta");
  meta.append(text("span", "",
    `${t("project.sessions", { n: project.sessionCount })} · ${relativeTime(project.lastActive)} `));
  for (const provider of project.providers) meta.append(agentBadge(provider));
  li.append(meta);
  if (project.lastPrompt) li.append(text("div", "prompt", project.lastPrompt));
  li.addEventListener("click", () => selectProject(project.realPath, li));
  return li;
}

// hiddenProjects entries may be glob patterns: "*" matches within one path
// segment, "**" across segments, "?" one character. Entries without "/"
// match the project's basename (gitignore-style); the rest match the full
// normalized path. Plain entries compare literally.
function globToRegExp(pattern) {
  let source = "";
  for (let i = 0; i < pattern.length; i++) {
    const ch = pattern[i];
    if (ch === "*") {
      if (pattern[i + 1] === "*") { source += ".*"; i++; }
      else source += "[^/]*";
    } else if (ch === "?") {
      source += "[^/]";
    } else {
      source += ch.replace(/[.+^${}()|[\]\\]/g, "\\$&");
    }
  }
  return new RegExp(`^${source}$`);
}

function hiddenEntryMatches(entry, project) {
  const subject = entry.includes("/") ? project.realPath : shortPath(project.realPath);
  if (/[*?]/.test(entry)) return globToRegExp(entry).test(subject);
  return subject === entry;
}

function isProjectHidden(project) {
  return state.config.hiddenProjects.some((entry) => hiddenEntryMatches(entry, project));
}

function renderProjects() {
  el.projects.replaceChildren();
  const hidden = [];
  for (const project of state.projects) {
    if (isProjectHidden(project)) hidden.push(project);
    else el.projects.append(projectRow(project, false));
  }
  if (hidden.length === 0) return;
  const toggle = text("li", "hidden-toggle",
    `${state.showHidden ? "▾" : "▸"} ${t("project.hiddenToggle", { n: hidden.length })}`);
  toggle.addEventListener("click", () => {
    state.showHidden = !state.showHidden;
    renderProjects();
  });
  el.projects.append(toggle);
  if (state.showHidden) {
    for (const project of hidden) el.projects.append(projectRow(project, true));
  }
}

async function hideProject(project) {
  if (!isProjectHidden(project)) {
    state.config.hiddenProjects = [...state.config.hiddenProjects, project.realPath];
  }
  renderProjects();
  await saveConfig();
}

// Inline ↩ removes every entry that matches the project. Removing a wildcard
// pattern can unhide siblings too — say so instead of leaving the user to
// wonder why three projects reappeared.
async function unhideProject(project) {
  const removedPatterns = state.config.hiddenProjects.filter(
    (entry) => hiddenEntryMatches(entry, project) && /[*?]/.test(entry));
  state.config.hiddenProjects = state.config.hiddenProjects.filter(
    (entry) => !hiddenEntryMatches(entry, project));
  if (removedPatterns.length > 0) {
    toast(t("toast.removedPatterns", { patterns: removedPatterns.join(", ") }), true);
  }
  renderProjects();
  await saveConfig();
}

async function removeHiddenEntry(entry) {
  state.config.hiddenProjects = state.config.hiddenProjects.filter((e) => e !== entry);
  renderProjects();
  await saveConfig();
}

async function addHiddenEntry(entry) {
  if (!entry || state.config.hiddenProjects.includes(entry)) return;
  state.config.hiddenProjects = [...state.config.hiddenProjects, entry];
  renderProjects();
  await saveConfig();
}

async function selectProject(path, li) {
  state.selectedProject = path;
  el.projects.querySelectorAll(".selected").forEach((n) => n.classList.remove("selected"));
  if (li) li.classList.add("selected");
  exitSearchMode();
  await loadSessions(path);
}

// ---- sessions (F2) ----------------------------------------------------------

// F2 continuous loading: one cheap handle fetch per project, then pages of
// summaries (parallel-parsed backend-side) as the list scrolls.
//
// Stale-while-revalidate, presentation-layer only: clicking a project
// renders the last-seen list instantly while the live read refreshes in the
// background. The backend stays stateless (ADR-2) — this memory dies with
// the window and is dropped whenever the data-source settings change.
const handleCache = new Map(); // project path -> RawSession[]
const summaryCache = new Map(); // "provider:nativeId@mtime" -> SessionSummary
const handleKey = (h) => `${h.providerId}:${h.nativeId}@${h.mtime}`;

const SESSION_PAGE = 50;
const sessionState = {
  handles: [],
  loaded: 0,
  generation: 0,
  sentinel: null,
  observer: null,
  loading: false,
  // Resume/fork chains: depth + root per session, descendants per root.
  depth: new Map(),
  rootOf: new Map(),
  chainSize: new Map(),
  expandedRoots: new Set(),
};

// Group resume/fork chains: a session whose parentNativeId is present in the
// project follows its root, indented; roots sort by the newest mtime in
// their chain. Chains collapse under the root by default.
function orderByChains(handles) {
  sessionState.depth = new Map();
  sessionState.rootOf = new Map();
  sessionState.chainSize = new Map();
  sessionState.expandedRoots = new Set();
  const byId = new Map(handles.map((h) => [h.nativeId, h]));
  const children = new Map();
  const roots = [];
  for (const handle of handles) {
    const pid = handle.parentNativeId;
    if (pid && pid !== handle.nativeId && byId.has(pid)) {
      if (!children.has(pid)) children.set(pid, []);
      children.get(pid).push(handle);
    } else {
      roots.push(handle);
    }
  }
  const newest = new Map();
  const chainNewest = (handle) => {
    if (newest.has(handle.nativeId)) return newest.get(handle.nativeId);
    let m = handle.mtime;
    for (const child of children.get(handle.nativeId) || []) {
      const cm = chainNewest(child);
      if (cm > m) m = cm;
    }
    newest.set(handle.nativeId, m);
    return m;
  };
  roots.sort((a, b) => (chainNewest(a) < chainNewest(b) ? 1 : -1));
  const ordered = [];
  const walk = (handle, depth, root) => {
    ordered.push(handle);
    sessionState.depth.set(handle.nativeId, depth);
    sessionState.rootOf.set(handle.nativeId, root);
    if (depth > 0) {
      sessionState.chainSize.set(root, (sessionState.chainSize.get(root) || 0) + 1);
    }
    const kids = (children.get(handle.nativeId) || [])
      .sort((a, b) => (a.mtime < b.mtime ? -1 : 1)); // segments chronologically
    for (const kid of kids) walk(kid, depth + 1, root);
  };
  for (const root of roots) walk(root, 0, root.nativeId);
  // Cyclic parent references (corrupt or adversarial data) would otherwise
  // swallow every involved session: demote unreached ones to roots.
  if (ordered.length < handles.length) {
    for (const handle of handles) {
      if (!sessionState.depth.has(handle.nativeId)) {
        ordered.push(handle);
        sessionState.depth.set(handle.nativeId, 0);
        sessionState.rootOf.set(handle.nativeId, handle.nativeId);
      }
    }
  }
  return ordered;
}

function toggleChain(rootId, badge) {
  const expanded = sessionState.expandedRoots.has(rootId);
  if (expanded) sessionState.expandedRoots.delete(rootId);
  else sessionState.expandedRoots.add(rootId);
  el.sessions
    .querySelectorAll(`li[data-chain-root="${CSS.escape(rootId)}"]`)
    .forEach((row) => row.classList.toggle("hidden", expanded));
  badge.textContent = chainBadgeLabel(rootId);
}

function chainBadgeLabel(rootId) {
  const count = sessionState.chainSize.get(rootId) || 0;
  const open = sessionState.expandedRoots.has(rootId);
  return `${open ? "▾" : "▸"} ⑂ ${count}`;
}

function sessionRow(session) {
  const li = document.createElement("li");
  li.dataset.id = session.id;
  // Re-renders (SWR refresh, project switch and back) must not lose the
  // current selection.
  if (session.id === state.selectedSession) li.classList.add("selected");
  const depth = sessionState.depth.get(session.nativeId) || 0;
  const rootId = sessionState.rootOf.get(session.nativeId);
  if (depth > 0) {
    li.classList.add("chain-child");
    li.dataset.chainRoot = rootId;
    if (!sessionState.expandedRoots.has(rootId)) li.classList.add("hidden");
  }
  li.append(text("div", "title", (depth > 0 ? "↳ " : "") + session.title));
  const branch = session.gitBranch ? ` · ${session.gitBranch}` : "";
  const meta = text("div", "meta");
  meta.append(agentBadge(session.providerId));
  meta.append(idChip(session.nativeId));
  if (depth === 0 && (sessionState.chainSize.get(session.nativeId) || 0) > 0) {
    const badge = text("span", "chain-badge", chainBadgeLabel(session.nativeId));
    badge.title = t("sessions.chainTitle");
    badge.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleChain(session.nativeId, badge);
    });
    meta.append(badge);
  }
  meta.append(text("span", "",
    ` ${t("sessions.meta", { time: relativeTime(session.mtime), n: session.messageCount, branch })}`));
  li.append(meta);
  li.addEventListener("click", () => selectSession(session.id, li));
  return li;
}

async function loadSessions(path) {
  const generation = ++sessionState.generation;
  const cached = handleCache.get(path);
  if (cached) {
    renderSessionList(cached, generation); // instant, from the last visit
  } else {
    el.sessions.replaceChildren(text("li", "notice", t("loading.sessions")));
  }
  const handles = await call("list_session_handles", { project: path });
  if (generation !== sessionState.generation) return; // superseded click
  handleCache.set(path, handles);
  // Revalidate: only re-render when the live read actually differs.
  if (!cached || JSON.stringify(cached) !== JSON.stringify(handles)) {
    renderSessionList(handles, generation);
  }
}

function renderSessionList(handles, generation) {
  if (generation !== sessionState.generation) return;
  el.sessions.replaceChildren();
  sessionState.handles = orderByChains(handles);
  sessionState.loaded = 0;
  sessionState.loading = false;
  if (sessionState.observer) sessionState.observer.disconnect();
  const sentinel = text("li", "notice", t("loading.generic"));
  sessionState.sentinel = sentinel;
  el.sessions.append(sentinel);
  sessionState.observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) loadNextSessionPage();
  }, { root: el.sessions, rootMargin: "400px" });
  sessionState.observer.observe(sentinel);
  loadNextSessionPage();
}

async function loadNextSessionPage() {
  if (sessionState.loading || sessionState.loaded >= sessionState.handles.length) return;
  sessionState.loading = true;
  const generation = sessionState.generation;
  const page = sessionState.handles.slice(
    sessionState.loaded, sessionState.loaded + SESSION_PAGE);
  // Summary cache is keyed on mtime, so an updated session is a natural
  // miss; only misses pay a backend parse.
  const missing = page.filter((handle) => !summaryCache.has(handleKey(handle)));
  if (missing.length > 0) {
    const fetched = await call("summarize_sessions", { handles: missing });
    if (generation !== sessionState.generation) return; // project changed mid-fetch
    for (const summary of fetched) {
      summaryCache.set(`${summary.providerId}:${summary.nativeId}@${summary.mtime}`, summary);
    }
  }
  const summaries = page
    .map((handle) => summaryCache.get(handleKey(handle)))
    .filter(Boolean); // vanished files (agents prune old sessions) drop out
  const fragment = document.createDocumentFragment();
  for (const session of summaries) fragment.append(sessionRow(session));
  el.sessions.insertBefore(fragment, sessionState.sentinel);
  sessionState.loaded += page.length;
  const remaining = sessionState.handles.length - sessionState.loaded;
  if (remaining > 0) {
    sessionState.sentinel.textContent = t("loading.more", { n: remaining });
  } else {
    sessionState.sentinel.classList.add("hidden");
    sessionState.observer.disconnect();
  }
  sessionState.loading = false;
  // The pane may still show the sentinel (short lists): keep filling.
  if (remaining > 0) {
    const rect = sessionState.sentinel.getBoundingClientRect();
    const pane = el.sessions.getBoundingClientRect();
    if (rect.top < pane.bottom + 200) loadNextSessionPage();
  }
}

async function selectSession(sessionId, li) {
  state.selectedSession = sessionId;
  el.sessions.querySelectorAll(".selected").forEach((n) => n.classList.remove("selected"));
  if (li) li.classList.add("selected");
  await loadDetail(sessionId);
}

// ---- detail timeline (F3) ---------------------------------------------------

// Transcript text is untrusted input: always parse with marked, then
// sanitize with DOMPurify before it touches the DOM.
function markdownNode(source) {
  const node = text("div", "block-text md");
  if (window.marked && window.DOMPurify) {
    node.innerHTML = DOMPurify.sanitize(
      marked.parse(source, { gfm: true, breaks: true, async: false }));
  } else {
    node.textContent = source;
  }
  return node;
}

function blockNode(block, context) {
  switch (block.kind) {
    case "text": {
      return markdownNode(block.text);
    }
    case "tool_use": {
      const details = text("details", "block tool");
      details.append(text("summary", "", t("detail.tool", { name: block.name })));
      details.append(text("pre", "", JSON.stringify(block.input, null, 2)));
      return details;
    }
    case "tool_result": {
      const details = text("details", "block result");
      details.append(text("summary", "",
        block.truncated ? t("detail.resultTruncated") : t("detail.result")));
      const pre = text("pre", "", block.summary);
      details.append(pre);
      if (block.truncated && context) {
        const load = text("button", "load-full", t("detail.loadFull"));
        load.addEventListener("click", async (event) => {
          event.preventDefault();
          load.disabled = true;
          load.textContent = t("loading.generic");
          try {
            const full = await invoke("get_message_full", {
              sessionId: context.sessionId,
              messageUuid: context.messageUuid,
            });
            const match = full && full.blocks && full.blocks[context.blockIndex];
            if (match && match.kind === "tool_result") {
              pre.textContent = match.summary;
              load.remove();
            } else {
              load.textContent = t("detail.fullUnavailable");
            }
          } catch (error) {
            toast(String(error));
            load.disabled = false;
            load.textContent = t("detail.loadFull");
          }
        });
        details.append(load);
      }
      return details;
    }
    case "thinking": {
      const details = text("details", "block thinking");
      details.append(text("summary", "", t("detail.thinking")));
      details.append(text("pre", "", block.text));
      return details;
    }
    default:
      return text("div", "block-text", "");
  }
}

async function loadDetail(sessionId) {
  // Only flag slow loads: dimming the pane for a 60ms parse would flicker.
  const pending = setTimeout(() => el.title.classList.add("loading"), 150);
  let session;
  try {
    session = await call("get_session", { sessionId });
  } finally {
    clearTimeout(pending);
    el.title.classList.remove("loading");
  }
  if (state.selectedSession !== sessionId) return; // superseded click
  const summary = session.summary;
  let sub = `${summary.projectPath} · ${t("detail.messages", { n: summary.messageCount })}`;
  const usage = session.extensions && session.extensions.usage;
  if (usage) {
    sub += ` · tokens ↑${compact(usage.inputTokens + usage.cacheCreationTokens + usage.cacheReadTokens)} ↓${compact(usage.outputTokens)}`;
  }
  el.title.classList.remove("placeholder");
  // Full id on its own line: long enough to matter, one click to copy.
  const idLine = text("span", "sub");
  idLine.append(agentBadge(summary.providerId));
  idLine.append(idChip(summary.nativeId, true));
  el.title.replaceChildren(
    text("span", "", summary.title),
    document.createElement("br"),
    idLine,
    document.createElement("br"),
    text("span", "sub", sub),
  );
  el.timeline.replaceChildren();
  const artifacts = session.extensions && session.extensions.artifacts;
  if (Array.isArray(artifacts) && artifacts.length > 0) {
    const box = text("div", "artifacts");
    box.append(text("div", "artifacts-title", t("detail.artifacts")));
    for (const artifact of artifacts) {
      const row = text("div", "artifact");
      row.append(text("span", "artifact-name", artifact.name));
      if (artifact.summary) row.append(text("span", "artifact-summary", artifact.summary));
      box.append(row);
    }
    el.timeline.append(box);
  }
  const subagents = session.extensions && session.extensions.subagents;
  if (Array.isArray(subagents) && subagents.length > 0) {
    const box = text("div", "artifacts");
    box.append(text("div", "artifacts-title", t("detail.subagents", { n: subagents.length })));
    for (const agent of subagents) {
      const details = text("details", "block tool subagent");
      const label = [agent.agentType, agent.description].filter(Boolean).join(" · ")
        || agent.agentId;
      details.append(text("summary", "",
        `⑂ ${label} (${t("detail.subagentMessages", { n: agent.messageCount })})`));
      const body = text("div", "subagent-messages");
      for (const m of agent.messages || []) {
        body.append(text("div", "subagent-line", `${m.role === "user" ? "❯" : "●"} ${m.preview}`));
      }
      details.append(body);
      box.append(details);
    }
    el.timeline.append(box);
  }
  startTimeline(session.messages);
  // Capability + path check only; no re-discovery on the backend.
  const resumable = await call("can_resume", {
    providerId: summary.providerId,
    projectPath: summary.projectPath,
  });
  el.resume.classList.toggle("hidden", !resumable);
  el.resume.onclick = async () => {
    const note = await call("resume_session", { sessionId });
    if (note) toast(note, true);
  };
}

// ---- chunked timeline rendering (F3: never DOM-render a 5MB session at once)

// Per-frame budget: markdown-parsing 200 messages in one go blocks the UI
// thread for hundreds of ms on prose-heavy sessions; 80 stays under a frame
// budget users notice, and the 600px rootMargin keeps scroll seamless.
const RENDER_CHUNK = 80;
const timelineState = { messages: [], rendered: 0, sentinel: null, observer: null };

function messageNode(message) {
  const row = text("div", `message ${message.role}`);
  row.dataset.uuid = message.uuid;
  row.append(text("div", "avatar", message.role === "user" ? "❯" : "●"));
  const body = text("div", "body");
  message.blocks.forEach((block, blockIndex) => {
    body.append(blockNode(block, {
      sessionId: state.selectedSession,
      messageUuid: message.uuid,
      blockIndex,
    }));
  });
  row.append(body);
  return row;
}

function renderNextChunk() {
  const end = Math.min(timelineState.rendered + RENDER_CHUNK, timelineState.messages.length);
  const fragment = document.createDocumentFragment();
  for (; timelineState.rendered < end; timelineState.rendered++) {
    fragment.append(messageNode(timelineState.messages[timelineState.rendered]));
  }
  el.timeline.insertBefore(fragment, timelineState.sentinel);
  const done = timelineState.rendered >= timelineState.messages.length;
  timelineState.sentinel.classList.toggle("hidden", done);
  if (done && timelineState.observer) timelineState.observer.disconnect();
}

function startTimeline(messages) {
  if (timelineState.observer) timelineState.observer.disconnect();
  timelineState.messages = messages;
  timelineState.rendered = 0;
  const sentinel = text("div", "timeline-sentinel", "…");
  timelineState.sentinel = sentinel;
  el.timeline.append(sentinel);
  timelineState.observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) renderNextChunk();
  }, { root: el.timeline, rootMargin: "600px" });
  timelineState.observer.observe(sentinel);
  renderNextChunk();
  if (state.scrollTarget) {
    // Render forward until the anchor exists, then jump to it.
    const index = messages.findIndex((m) => m.uuid === state.scrollTarget);
    while (index >= 0 && timelineState.rendered <= index) renderNextChunk();
    const target = el.timeline.querySelector(`[data-uuid="${CSS.escape(state.scrollTarget)}"]`);
    if (target) {
      target.scrollIntoView({ block: "start" });
      target.classList.add("highlight");
    }
    state.scrollTarget = null;
  } else {
    el.timeline.scrollTop = 0;
  }
}

// ---- search (F4) --------------------------------------------------------------

function exitSearchMode() {
  el.results.classList.add("hidden");
  el.sessions.classList.remove("hidden");
}

let searchGeneration = 0;
async function runSearch() {
  const query = el.search.value.trim();
  if (!query) {
    exitSearchMode();
    return;
  }
  const generation = ++searchGeneration;
  el.results.replaceChildren(text("li", "empty", t("search.searching")));
  el.results.classList.remove("hidden");
  el.sessions.classList.add("hidden");
  const hits = await call("search", { query, project: state.selectedProject });
  if (generation !== searchGeneration) return; // superseded query
  el.results.replaceChildren();
  if (hits.length === 0) {
    el.results.append(text("li", "empty", t("search.noMatches", { query })));
    return;
  }
  // F4: results aggregated per session.
  const groups = new Map();
  for (const hit of hits) {
    if (!groups.has(hit.sessionId)) groups.set(hit.sessionId, []);
    groups.get(hit.sessionId).push(hit);
  }
  for (const [sessionId, sessionHits] of groups) {
    const first = sessionHits[0];
    const header = text("li", "group-header");
    header.append(agentBadge(first.providerId));
    header.append(text("span", "", ` ${first.projectPath} · ${first.nativeSessionId.slice(0, 8)}`));
    el.results.append(header);
    for (const hit of sessionHits.slice(0, 5)) {
      const li = text("li", "hit");
      li.append(text("div", "snippet", hit.snippet));
      li.addEventListener("click", () => openHit(hit, li));
      el.results.append(li);
    }
  }
}

async function openHit(hit, li) {
  // Open the hit's session in the detail pane, anchored on the matched
  // message — but keep the results list: browsing through several hits is
  // the whole point of a search. Escape / clearing the query exits.
  el.results.querySelectorAll(".selected").forEach((n) => n.classList.remove("selected"));
  if (li) li.classList.add("selected");
  state.scrollTarget = hit.messageUuid || null;
  state.selectedSession = hit.sessionId;
  await loadDetail(hit.sessionId);
}

// ---- settings ----------------------------------------------------------------
// Grouped by engineering-tool dimensions (TECH_SPEC §12): data sources /
// resume workflow / workspace, with the config file itself as the footer.
// The pane is a thin editor over config.json — never a second source of truth.

async function openSettings() {
  el.settingsOverlay.classList.remove("hidden");
  await renderSettings();
}

function closeSettings() {
  el.settingsOverlay.classList.add("hidden");
}

async function renderSettings() {
  renderHiddenProjects();
  try {
    const info = await invoke("settings_info");
    el.configPath.textContent = info.path;
    el.settingsProviders.replaceChildren();
    for (const provider of info.providers) {
      el.settingsProviders.append(providerSettingsRow(provider));
    }
  } catch (error) {
    toast(String(error));
  }
}

function providerStatusChip(provider) {
  const kind = !provider.enabled ? "off" : provider.rootExists ? "ok" : "bad";
  const label = !provider.enabled
    ? t("settings.status.disabled")
    : provider.rootExists
      ? t("settings.status.found")
      : t("settings.status.missing");
  const chip = text("span", `provider-status ${kind}`, label);
  chip.title = provider.enabled
    ? (provider.rootExists
        ? t("settings.status.foundTitle", { root: provider.root })
        : t("settings.status.missingTitle", { root: provider.root }))
    : t("settings.status.disabledTitle");
  return chip;
}

function providerSettingsRow(provider) {
  const row = text("div", "provider-row");
  const head = text("label", "settings-row");
  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.checked = provider.enabled;
  head.append(toggle);
  head.append(text("span", "settings-label", provider.name));
  head.append(providerStatusChip(provider));
  row.append(head);
  const root = document.createElement("input");
  root.type = "text";
  root.className = "provider-root";
  root.spellcheck = false;
  root.placeholder = provider.defaultRoot;
  root.value = provider.rootIsCustom ? provider.root : "";
  root.title = t("settings.rootTitle");
  toggle.addEventListener("change", () =>
    updateProviderSetting(provider.id, { enabled: toggle.checked }));
  root.addEventListener("change", () =>
    updateProviderSetting(provider.id, { root: root.value.trim() || null }));
  row.append(root);
  return row;
}

async function updateProviderSetting(id, patch) {
  const current = state.config.providers[id] || { enabled: true, root: null };
  state.config.providers = {
    ...state.config.providers,
    [id]: { ...current, ...patch },
  };
  await saveConfig();
  handleCache.clear(); // the set of data sources changed: stale by definition
  summaryCache.clear();
  await renderSettings(); // status chips re-validate backend-side
  await loadProjects();
}

function renderHiddenProjects() {
  el.settingsHidden.replaceChildren();
  const hidden = state.config.hiddenProjects;
  if (hidden.length === 0) {
    el.settingsHidden.append(text("p", "settings-note", t("settings.noHidden")));
  }
  for (const entry of hidden) {
    const row = text("div", "hidden-row");
    const label = text("span", "hidden-path", entry);
    label.title = entry;
    row.append(label);
    if (/[*?]/.test(entry)) {
      const count = state.projects.filter((p) => hiddenEntryMatches(entry, p)).length;
      row.append(text("span", "hidden-count", t("settings.matches", { n: count })));
    }
    const unhide = text("button", "unhide-btn", t("settings.unhide"));
    unhide.addEventListener("click", async () => {
      await removeHiddenEntry(entry);
      renderHiddenProjects();
    });
    row.append(unhide);
    el.settingsHidden.append(row);
  }
  // Free-form entry: the only way to hide by wildcard. "*" within a path
  // segment, "**" across, "?" one char; no "/" → match the project name.
  const form = text("div", "hidden-add");
  const input = document.createElement("input");
  input.type = "text";
  input.spellcheck = false;
  input.placeholder = t("settings.hidePlaceholder");
  input.addEventListener("keydown", async (event) => {
    if (event.key !== "Enter") return;
    await addHiddenEntry(input.value.trim());
    renderHiddenProjects();
  });
  form.append(input);
  el.settingsHidden.append(form);
}

el.settingsBtn.addEventListener("click", openSettings);
// Native menu bar "Settings…" (⌘,) — the Rust shell emits this event.
if (window.__TAURI__.event) {
  window.__TAURI__.event.listen("open-settings", () => openSettings());
}
el.settingsClose.addEventListener("click", closeSettings);
el.settingsOverlay.addEventListener("click", (event) => {
  if (event.target === el.settingsOverlay) closeSettings();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !el.settingsOverlay.classList.contains("hidden")) {
    closeSettings();
  }
});
el.configReveal.addEventListener("click", () => call("reveal_config"));

// ---- boot --------------------------------------------------------------------

el.search.addEventListener("keydown", (event) => {
  if (event.key === "Enter") runSearch();
  if (event.key === "Escape") {
    el.search.value = "";
    exitSearchMode();
  }
});

async function initConfig() {
  try {
    const config = await invoke("get_config");
    state.config = { hiddenProjects: [], providers: {}, language: "auto", ...config };
    el.terminal.value = state.config.terminal;
  } catch (_) { /* config is optional */ }
  I18N.setLanguage(state.config.language || "auto");
  el.language.value = state.config.language || "auto";
  el.terminal.addEventListener("change", () => {
    state.config.terminal = el.terminal.value;
    saveConfig();
  });
  // Language switch reloads the window: every rendered string re-derives
  // from the dictionary, and a reload is simpler and safer than re-rendering
  // every live view in place.
  el.language.addEventListener("change", async () => {
    state.config.language = el.language.value;
    await saveConfig();
    window.location.reload();
  });
}

function saveConfig() {
  return call("set_config", { config: state.config });
}

// Surface runtime errors — a silent exception reads as "clicks do nothing".
window.addEventListener("error", (event) => {
  toast(`JS error: ${event.message} (${(event.filename || "").split("/").pop()}:${event.lineno})`);
});
window.addEventListener("unhandledrejection", (event) => {
  toast(`Unhandled rejection: ${event.reason}`);
});

// Links inside rendered markdown must not navigate the webview; hand them
// to the system browser instead.
el.timeline.addEventListener("click", (event) => {
  const link = event.target.closest("a[href]");
  if (!link) return;
  event.preventDefault();
  call("open_external", { url: link.href });
});

// Config first: the project list needs hiddenProjects before it renders.
initConfig().then(loadProjects);
