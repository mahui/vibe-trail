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
};

const state = {
  selectedProject: null,
  selectedSession: null,
  scrollTarget: null,
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
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  if (seconds < 86400 * 30) return `${Math.floor(seconds / 86400)}d ago`;
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
    toast(`Session id copied: ${nativeId}`, true);
  } catch (error) {
    toast(String(error));
  }
}

function idChip(nativeId, full = false) {
  const chip = text("span", "id-chip", full ? nativeId : nativeId.slice(0, 8));
  chip.title = `${nativeId}\nClick to copy`;
  chip.addEventListener("click", (event) => {
    event.stopPropagation();
    copySessionId(nativeId);
  });
  return chip;
}

function providerLabel(id) {
  return id === "antigravity" ? "antigravity (exp)" : id;
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
  const projects = await call("list_projects");
  el.projects.replaceChildren();
  for (const project of projects) {
    const li = document.createElement("li");
    li.dataset.path = project.realPath;
    const name = text("div", "name" + (project.exists ? "" : " orphaned"), shortPath(project.realPath));
    if (!project.exists) name.append(text("span", "", "⚠"));
    li.append(name);
    li.append(text("div", "meta",
      `${project.sessionCount} sessions · ${relativeTime(project.lastActive)} · ${[...project.providers].map(providerLabel).join(",")}`));
    if (project.lastPrompt) li.append(text("div", "prompt", project.lastPrompt));
    li.addEventListener("click", () => selectProject(project.realPath, li));
    el.projects.append(li);
  }
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
const SESSION_PAGE = 50;
const sessionState = {
  handles: [],
  loaded: 0,
  generation: 0,
  sentinel: null,
  observer: null,
  loading: false,
};

function sessionRow(session) {
  const li = document.createElement("li");
  li.dataset.id = session.id;
  li.append(text("div", "title", session.title));
  const branch = session.gitBranch ? ` · ${session.gitBranch}` : "";
  const meta = text("div", "meta");
  meta.append(idChip(session.nativeId));
  meta.append(text("span", "",
    ` ${providerLabel(session.providerId)} · ${relativeTime(session.mtime)} · ${session.messageCount} msg${branch}`));
  li.append(meta);
  li.addEventListener("click", () => selectSession(session.id, li));
  return li;
}

async function loadSessions(path) {
  const generation = ++sessionState.generation;
  el.sessions.replaceChildren();
  const handles = await call("list_session_handles", { project: path });
  if (generation !== sessionState.generation) return; // superseded click
  sessionState.handles = handles;
  sessionState.loaded = 0;
  sessionState.loading = false;
  if (sessionState.observer) sessionState.observer.disconnect();
  const sentinel = text("li", "notice", "Loading…");
  sessionState.sentinel = sentinel;
  el.sessions.append(sentinel);
  sessionState.observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) loadNextSessionPage();
  }, { root: el.sessions, rootMargin: "400px" });
  sessionState.observer.observe(sentinel);
  await loadNextSessionPage();
}

async function loadNextSessionPage() {
  if (sessionState.loading || sessionState.loaded >= sessionState.handles.length) return;
  sessionState.loading = true;
  const generation = sessionState.generation;
  const page = sessionState.handles.slice(
    sessionState.loaded, sessionState.loaded + SESSION_PAGE);
  const summaries = await call("summarize_sessions", { handles: page });
  if (generation !== sessionState.generation) return; // project changed mid-fetch
  const fragment = document.createDocumentFragment();
  for (const session of summaries) fragment.append(sessionRow(session));
  el.sessions.insertBefore(fragment, sessionState.sentinel);
  sessionState.loaded += page.length;
  const remaining = sessionState.handles.length - sessionState.loaded;
  if (remaining > 0) {
    sessionState.sentinel.textContent = `Loading… (${remaining} more)`;
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

function blockNode(block) {
  switch (block.kind) {
    case "text": {
      return markdownNode(block.text);
    }
    case "tool_use": {
      const details = text("details", "block tool");
      details.append(text("summary", "", `Tool: ${block.name}`));
      details.append(text("pre", "", JSON.stringify(block.input, null, 2)));
      return details;
    }
    case "tool_result": {
      const details = text("details", "block result");
      details.append(text("summary", "", `Result${block.truncated ? " (truncated)" : ""}`));
      details.append(text("pre", "", block.summary));
      return details;
    }
    case "thinking": {
      const details = text("details", "block thinking");
      details.append(text("summary", "", "Thinking"));
      details.append(text("pre", "", block.text));
      return details;
    }
    default:
      return text("div", "block-text", "");
  }
}

async function loadDetail(sessionId) {
  const session = await call("get_session", { sessionId });
  const summary = session.summary;
  let sub = `${summary.projectPath} · ${summary.messageCount} messages`;
  const usage = session.extensions && session.extensions.usage;
  if (usage) {
    sub += ` · tokens ↑${compact(usage.inputTokens + usage.cacheCreationTokens + usage.cacheReadTokens)} ↓${compact(usage.outputTokens)}`;
  }
  el.title.classList.remove("placeholder");
  // Full id on its own line: long enough to matter, one click to copy.
  const idLine = text("span", "sub");
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
    box.append(text("div", "artifacts-title", "Artifacts"));
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
    box.append(text("div", "artifacts-title", `Subagents (${subagents.length})`));
    for (const agent of subagents) {
      const details = text("details", "block tool subagent");
      const label = [agent.agentType, agent.description].filter(Boolean).join(" · ")
        || agent.agentId;
      details.append(text("summary", "", `⑂ ${label} (${agent.messageCount} messages)`));
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

const RENDER_CHUNK = 200;
const timelineState = { messages: [], rendered: 0, sentinel: null, observer: null };

function messageNode(message) {
  const row = text("div", `message ${message.role}`);
  row.dataset.uuid = message.uuid;
  row.append(text("div", "avatar", message.role === "user" ? "❯" : "●"));
  const body = text("div", "body");
  for (const block of message.blocks) body.append(blockNode(block));
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

async function runSearch() {
  const query = el.search.value.trim();
  if (!query) {
    exitSearchMode();
    return;
  }
  el.results.replaceChildren(text("li", "empty", "Searching…"));
  el.results.classList.remove("hidden");
  el.sessions.classList.add("hidden");
  const hits = await call("search", { query, project: state.selectedProject });
  el.results.replaceChildren();
  if (hits.length === 0) {
    el.results.append(text("li", "empty", `No matches for “${query}”.`));
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
    el.results.append(text("li", "group-header",
      `${first.projectPath} · ${first.nativeSessionId.slice(0, 8)}`));
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
    el.terminal.value = config.terminal;
  } catch (_) { /* config is optional */ }
  el.terminal.addEventListener("change", () => {
    call("set_config", { config: { terminal: el.terminal.value } });
  });
}

initConfig();
// Links inside rendered markdown must not navigate the webview; hand them
// to the system browser instead.
el.timeline.addEventListener("click", (event) => {
  const link = event.target.closest("a[href]");
  if (!link) return;
  event.preventDefault();
  call("open_external", { url: link.href });
});

loadProjects();
