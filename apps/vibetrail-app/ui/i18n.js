// Frontend i18n, hand-rolled: a flat two-locale dictionary plus a tiny t()
// — no build chain, no library (ADR-1 front-end discipline). Dictionary
// values are strings with {x} placeholders, or functions for plural-sensitive
// English. Backend-produced text (errors, resume notes) stays English; the
// boundary is documented in TECH_SPEC §7.
"use strict";

const I18N = (() => {
  const MESSAGES = {
    en: {
      "app.projects": "Projects",
      "app.settingsTitle": "Settings",
      "app.searchPlaceholder": "Search all sessions…",
      "app.selectSession": "Select a session",
      "app.resume": "▶ Resume",
      "loading.projects": "Loading projects…",
      "loading.sessions": "Loading sessions…",
      "loading.generic": "Loading…",
      "loading.more": (p) => `Loading… (${p.n} more)`,
      "time.justNow": "just now",
      "time.minutesAgo": "{n}m ago",
      "time.hoursAgo": "{n}h ago",
      "time.daysAgo": "{n}d ago",
      "toast.idCopied": "Session id copied: {id}",
      "toast.removedPatterns": (p) =>
        `Removed pattern${p.patterns.includes(",") ? "s" : ""}: ${p.patterns}`,
      "chip.copyTitle": "Click to copy",
      "project.hide": "Hide project",
      "project.unhide": "Unhide project",
      "project.hiddenToggle": (p) =>
        `${p.n} hidden project${p.n > 1 ? "s" : ""}`,
      "project.sessions": (p) => `${p.n} sessions`,
      "sessions.chainTitle": "Resume/fork continuations — click to expand",
      "sessions.meta": (p) => `${p.time} · ${p.n} msg${p.branch}`,
      "search.searching": "Searching…",
      "search.noMatches": "No matches for “{query}”.",
      "detail.messages": "{n} messages",
      "detail.tool": "Tool: {name}",
      "detail.result": "Result",
      "detail.resultTruncated": "Result (truncated)",
      "detail.thinking": "Thinking",
      "detail.loadFull": "Load full output",
      "detail.fullUnavailable": "Full output unavailable",
      "detail.artifacts": "Artifacts",
      "detail.subagents": "Subagents ({n})",
      "detail.subagentMessages": (p) => `${p.n} messages`,
      "settings.title": "Settings",
      "settings.close": "Close",
      "settings.dataSources": "Data sources",
      "settings.dataSourcesNote":
        "Where sessions are discovered. Agent stores are always read-only; leave the root empty to use the default. Applies to the CLI too.",
      "settings.resumeSection": "Resume",
      "settings.terminal": "Terminal",
      "settings.terminalTitle": "Terminal used by Resume",
      "settings.warpNote":
        "Warp can't run commands scriptably: VibeTrail opens the project and copies the resume command to the clipboard instead.",
      "settings.language": "Language",
      "settings.languageAuto": "Auto (system)",
      "settings.workspace": "Workspace",
      "settings.reveal": "Reveal in Finder",
      "settings.configTitle": "Settings are stored as plain, hand-editable JSON",
      "settings.rootTitle": "Store root override (~ allowed); empty = default",
      "settings.status.disabled": "disabled",
      "settings.status.found": "found",
      "settings.status.missing": "missing",
      "settings.status.disabledTitle":
        "Excluded from discovery, search and resume (GUI and CLI)",
      "settings.status.foundTitle": "Store root exists: {root}",
      "settings.status.missingTitle": "Store root does not exist: {root}",
      "settings.noHidden":
        "No hidden projects. Hover a project in the sidebar and click ⊘ to hide it, or add a pattern below.",
      "settings.matches": (p) => `${p.n} match${p.n === 1 ? "" : "es"}`,
      "settings.unhide": "Unhide",
      "settings.hidePlaceholder":
        "Hide by path or pattern, e.g. **/scratch/** or tmp-*",
    },
    zh: {
      "app.projects": "项目",
      "app.settingsTitle": "设置",
      "app.searchPlaceholder": "搜索全部会话…",
      "app.selectSession": "选择一个会话",
      "app.resume": "▶ 恢复",
      "loading.projects": "正在加载项目…",
      "loading.sessions": "正在加载会话…",
      "loading.generic": "加载中…",
      "loading.more": "加载中…（还有 {n} 条）",
      "time.justNow": "刚刚",
      "time.minutesAgo": "{n} 分钟前",
      "time.hoursAgo": "{n} 小时前",
      "time.daysAgo": "{n} 天前",
      "toast.idCopied": "会话 id 已复制：{id}",
      "toast.removedPatterns": "已移除通配符：{patterns}",
      "chip.copyTitle": "点击复制",
      "project.hide": "隐藏项目",
      "project.unhide": "取消隐藏",
      "project.hiddenToggle": "{n} 个隐藏项目",
      "project.sessions": "{n} 个会话",
      "sessions.chainTitle": "恢复/分叉的后续会话——点击展开",
      "sessions.meta": (p) => `${p.time} · ${p.n} 条${p.branch}`,
      "search.searching": "搜索中…",
      "search.noMatches": "没有与“{query}”匹配的结果。",
      "detail.messages": "{n} 条消息",
      "detail.tool": "工具：{name}",
      "detail.result": "结果",
      "detail.resultTruncated": "结果（已截断）",
      "detail.thinking": "思考",
      "detail.loadFull": "加载完整输出",
      "detail.fullUnavailable": "完整输出不可用",
      "detail.artifacts": "产物",
      "detail.subagents": "子代理（{n}）",
      "detail.subagentMessages": "{n} 条消息",
      "settings.title": "设置",
      "settings.close": "关闭",
      "settings.dataSources": "数据源",
      "settings.dataSourcesNote":
        "会话的发现位置。agent 存储始终只读；根路径留空即使用默认值。对 CLI 同样生效。",
      "settings.resumeSection": "恢复",
      "settings.terminal": "终端",
      "settings.terminalTitle": "恢复会话所用的终端",
      "settings.warpNote":
        "Warp 无法脚本化执行命令：VibeTrail 会打开项目目录，并把恢复命令复制到剪贴板。",
      "settings.language": "语言",
      "settings.languageAuto": "跟随系统",
      "settings.workspace": "工作区",
      "settings.reveal": "在 Finder 中显示",
      "settings.configTitle": "设置以纯 JSON 文件存储，可手工编辑",
      "settings.rootTitle": "存储根路径覆盖（支持 ~）；留空使用默认",
      "settings.status.disabled": "已禁用",
      "settings.status.found": "已找到",
      "settings.status.missing": "缺失",
      "settings.status.disabledTitle": "从发现、搜索与恢复中排除（GUI 与 CLI）",
      "settings.status.foundTitle": "存储根路径存在：{root}",
      "settings.status.missingTitle": "存储根路径不存在：{root}",
      "settings.noHidden":
        "暂无隐藏项目。将鼠标悬停在侧栏项目上点 ⊘ 隐藏，或在下方添加通配符。",
      "settings.matches": "命中 {n} 个",
      "settings.unhide": "取消隐藏",
      "settings.hidePlaceholder": "按路径或通配符隐藏，例如 **/scratch/** 或 tmp-*",
    },
  };

  let lang = "en";

  function resolve(preference) {
    if (preference === "en" || preference === "zh") return preference;
    return (navigator.language || "en").toLowerCase().startsWith("zh")
      ? "zh"
      : "en";
  }

  function t(key, params) {
    const entry = MESSAGES[lang][key] ?? MESSAGES.en[key];
    if (entry === undefined) return key;
    if (typeof entry === "function") return entry(params || {});
    if (!params) return entry;
    return entry.replace(/\{(\w+)\}/g, (_, name) =>
      params[name] !== undefined ? String(params[name]) : `{${name}}`
    );
  }

  /// Fill every element carrying data-i18n / data-i18n-title /
  /// data-i18n-placeholder from the active dictionary.
  function applyStatic() {
    document.documentElement.lang = lang;
    for (const node of document.querySelectorAll("[data-i18n]")) {
      node.textContent = t(node.dataset.i18n);
    }
    for (const node of document.querySelectorAll("[data-i18n-title]")) {
      node.title = t(node.dataset.i18nTitle);
    }
    for (const node of document.querySelectorAll("[data-i18n-placeholder]")) {
      node.placeholder = t(node.dataset.i18nPlaceholder);
    }
  }

  return {
    t,
    resolve,
    applyStatic,
    setLanguage(preference) {
      lang = resolve(preference);
      applyStatic();
    },
    get lang() {
      return lang;
    },
  };
})();

const t = I18N.t;
