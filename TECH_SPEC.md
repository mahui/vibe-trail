# 技术设计 — VibeTrail

**版本:** v0.3
**日期:** 2026-07-02
**配套文档:** PRD.md

---

## 1. 总体架构

一份 Core,两个薄壳,N 个 Provider。所有业务逻辑在 Core;GUI 与 CLI 仅做调用与呈现;每个 agent 生态的差异被 Provider trait 隔离在各自实现内。

```
vibetrail/                        # Cargo workspace
├── crates/
│   ├── vibetrail-core/           # 统一模型、Provider trait、搜索、resume 编排
│   │   ├── src/providers/
│   │   │   ├── claude_code/      # v1
│   │   │   ├── codex/            # v1.1
│   │   │   ├── antigravity/      # v1.2, experimental
│   │   │   ├── cursor/           # v1.3, experimental(SQLite 只读,ADR-7)
│   │   │   └── qoder/            # v1.4
│   │   └── tests/                # 各 provider fixture 对拍
│   └── vibetrail-cli/            # CLI 薄壳(clap)
└── apps/
    └── vibetrail-app/            # Tauri v2 app
        ├── src-tauri/            # Rust 薄壳(tauri commands → core)
        └── ui/                   # 静态前端(vanilla HTML/CSS/JS,无 Node 构建链)
```

无 HTTP server、无数据库、无常驻进程、无 FS watcher。

## 2. ADR 摘要

### ADR-1:技术栈 Rust workspace + Tauri(取代 Swift/SwiftUI)

**状态:** 已接受(v0.3 修订;v0.2 曾选 Swift/SwiftUI 并完成 M1–M3,本版整体迁移)
**理由:** 单语言贯穿 Core/CLI/GUI;ripgrep 生态可直接 link crate(见 ADR-3);保留跨平台自由度,开源社区贡献门槛更低。
**代价:** GUI 为 WebView 而非原生质感;与 VibeSpace/Pier 产品线不同栈。
**备选:** Swift/SwiftUI(v0.2 实现,已被本版替换)。
**前端纪律:** v1 前端为纯静态 HTML/CSS/JS,不引入 Node 构建链;复杂度增长后再评估框架。

### ADR-2:活读文件,不建索引

**状态:** 已接受
**理由:** 数百 session / 数百 MB 规模下目录扫描 + grep 引擎均在秒级内;无索引则无一致性问题、无后台进程。
**升级路径:** `SearchEngine` trait 后加 FTS5 缓存实现,接口不变。

### ADR-3:搜索 link ripgrep 引擎 crates

**状态:** 已接受(v0.3 修订;v0.2 因 Swift 无法 link crate 而 shell out)
**实现:** 直接依赖 `grep-searcher` / `grep-regex` / `walkdir`,库内联搜索,无外部 `rg` 二进制依赖,Homebrew formula 不再声明 ripgrep。固定字符串、大小写不敏感。压缩会话(Codex .zst)由 provider 解压后喂给同一引擎或降级为 provider 内搜索。

### ADR-4:Resume 实现

**状态:** 已接受
**CLI:** 校验路径后 `chdir` 到项目路径并 `exec` provider 给出的 resume 命令(Unix `CommandExt::exec`)。
**GUI:** Tauri 后端(Rust)按用户配置的终端拉起执行(P1 已交付,配置在 `~/.config/vibetrail/config.json`):Terminal.app / iTerm2 经 `osascript` 直接执行;Warp 无可脚本化的"执行命令"面,降级为打开项目目录 + resume 命令入剪贴板并提示用户粘贴。
**Ghostty 教训(2026-07):** 其 AppleScript 字典(1.3)是官方声明的 preview——实测连续 resume 会把 Ghostty 驱动到崩溃,已撤回依赖。现行为:未运行 → `open -a`(不带 `-n`)冷启动传参,单实例无重复图标,命令尾接 `exec $SHELL` 保持交互;已运行 → **NSPerformService 触发其 Finder service "New Ghostty Tab Here"**(Info.plist NSServices/openTab——Finder 级成熟 handler,经 JXA 桥调用,零 AppleEvent 零 TCC),在项目目录开好新 tab + resume 命令入剪贴板,用户仅剩粘贴回车;service 不可用时退回激活+剪贴板。等 Ghostty 1.4 scripting 稳定后再评估全自动执行。禁止用 `open -n`(每次 resume 多一个 Dock 图标)、System Events keystroke(需辅助功能权限,且中文输入法下键击注入乱码)与 preview scripting 字典。
**客户端 App 形态(2026-07,Cursor):** resume 目标不总是终端命令——Cursor 的会话属于其 GUI 客户端,"resume"语义是打开 Cursor 到对应项目窗口、由用户在聊天历史面板回到会话。`ResumeSpec` 增加启动模式 `launch: Terminal | GuiApp`(默认 Terminal,既有 provider 行为不变):GuiApp 不经终端配置,两壳直接 detached spawn 命令(CLI spawn 后退出 0;GUI 不走 osascript/终端链路),Cursor 的命令即 `["open", "-a", "Cursor", "<project_path>"]`(单实例,同窗口复用,无重复 Dock 图标问题)。会话级直达无公开接口(官方 deeplink 仅 prompt 预填与 MCP 安装,"链接历史会话"在官方论坛仍是 feature request),resume 时 UI 展示会话标题辅助用户在 Cursor 历史面板定位;将来官方提供 composer deeplink 再升级直达。
**安全:** 无网络监听面;AppleScript 需 Automation 权限,首次触发引导授权。config.json 是 VibeTrail 唯一写入的文件,agent 存储目录仍严格只读。

### ADR-5:License 与开源结构

**状态:** 已接受(2026-07 拍板)
**决定:** Apache-2.0,单 repo 全开源。选 Apache-2.0 而非 MIT:自带专利授权条款,对企业使用者更友好;与核心依赖(Tauri、ripgrep 引擎 crates)许可兼容。
**备选(未采纳):** open core(Core+CLI 开源,App 闭源收费)。当前判断:先全开源换社区与 star,付费能力(语义搜索等)出现时再评估拆分;Core/壳边界从第一天保持干净,保留拆分自由度。

### ADR-6:Provider 抽象与准入原则

**状态:** 已接受
**原则:** 只接纳纯文件读取可覆盖的能力。需要宿主进程存活(如 Antigravity LanguageServer API)或逆向无 schema 私有格式(.pb)的能力一律不做或降级,不为最弱 provider 污染"零依赖活读"的架构承诺。
**纪律:** 抽象第一天建立,但 v1 只 ship Claude Code 一个实现;Codex(v1.1)的用途是验证抽象切分是否正确——单实现的抽象是猜测,两个实现才算数。

### ADR-7:SQLite 只读读取准入(Cursor)

**状态:** 已接受(2026-07)
**背景:** Cursor IDE 的会话存储是 SQLite(state.vscdb)内嵌 JSON,不是 JSONL 平文件。
**决定:** ADR-6"纯文件读取"的判据是**不需要宿主进程存活、格式可稳定解析**,而非文件后缀。SQLite 是有公开规范的开放文件格式,只读打开与读 JSONL 性质相同,准入。ADR-2 / 铁律里的"无数据库"约束的是 VibeTrail 不自建存储与索引;provider 自身恰好用 SQLite 存数据,只读解析它不在此列。
**代价与边界:** Core 引入 `rusqlite`(bundled 静态链接,无外部依赖,许可兼容 Apache-2.0)。只读纪律见 §4.4。逆向无 schema 私有格式(.pb)与宿主进程 API 依赖仍然拒收——ADR-6 原则不变,本条只澄清边界。

## 3. Provider 协议

```rust
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;         // "claude-code" / "codex" / "antigravity"
    fn capabilities(&self) -> ProviderCapabilities;
    fn discover(&self) -> Result<Vec<RawSession>>;           // 枚举存储,轻量(元数据级)
    fn parse(&self, raw: &RawSession) -> Result<Session>;    // 归一化到统一模型
    fn outline(&self, raw: &RawSession) -> Result<Vec<MessageStub>>; // 懒加载
    fn page(&self, raw: &RawSession, offset: usize, limit: usize) -> Result<Vec<Message>>;
    fn resume_spec(&self, raw: &RawSession) -> Option<ResumeSpec>; // None = 不可 resume;只依赖元数据,禁止全量 parse
    fn quick_title(&self, raw: &RawSession) -> Option<String>; // 元数据级标题提取(项目总览用);默认实现回退全量 parse
    fn find(&self, reference: &str) -> Result<Vec<RawSession>>; // 按 native id/前缀定位;默认 discover+filter,id 在文件名里的 provider(Codex)覆写为纯目录走查
    fn message_full(&self, raw: &RawSession, message_uuid: &str) -> Result<Option<Message>>; // 单条消息的未截断版本,按需重读磁盘(tool result 展示层截断在 2000 字符)
    fn summarize(&self, raw: &RawSession) -> Result<SessionSummary>; // 默认实现 = parse().summary
    // 搜索适配(可 grep 的 provider 覆写;默认空 → 该 provider 不参与全文搜索)
    fn search_roots(&self, project_path: Option<&str>) -> Vec<PathBuf>;
    fn resolve_hit(&self, file: &Path, line_number: u64, line: &str, query: &str) -> Option<SearchHit>;
    fn search_compressed(&self, query: &str, project_path: Option<&str>) -> Vec<SearchHit>; // ADR-3 降级路径,默认空
}

pub struct ProviderCapabilities {
    pub resumable: bool,        // CC ✓ / Codex ✓ / AGY ✗ / Cursor ✓(客户端级,见 ADR-4)
    pub file_based_only: bool,  // CC ✓ / Codex ✓ / AGY 部分 / Cursor ✓(SQLite,ADR-7)
    pub has_artifacts: bool,    // AGY ✓(plan/task/walkthrough)
    pub project_native: bool,   // CC ✓ / Codex ✗(按日期) / AGY ✗(按 conversation) / Cursor ✗(按 workspace 映射)
}
```

搜索引擎对 `search_roots` 做库内并行 grep,把命中行(含 1-based 行号)交回所属 provider 的 `resolve_hit` 解析出 session/message 定位——格式知识不出 provider。消息无内在 id 的 provider(Codex)用 `L<行号>` 作为 message uuid 锚点。存储无法按项目收窄的 provider(Codex 按日期组织)返回全量 roots,引擎在解析后按 scope 统一过滤 project_path。压缩会话走 `search_compressed` 降级路径。发现与搜索均用 rayon 并行 I/O——仍无索引、无缓存(ADR-2 不变)。

- **项目是派生属性,不是存储属性。** 各 provider 从元数据提取 cwd,Core 归一化(展开 ~、resolve symlink)后聚合分组。CC 的目录结构只是恰好预分好组。
- **统一 `Session` 模型取最小公倍数**(消息序列、角色、时间戳、cwd、provider id)+ provider 扩展字段(`extensions: [String: Codable]`)。CC 的 subagent 树、AGY 的 artifacts 走扩展字段,不塞进通用模型。
- UI 按 capabilities 降级:不可 resume 隐藏按钮;hasArtifacts 显示附件区。

## 4. 各 Provider 数据源规范

### 4.1 Claude Code(v1)

**存储:**

```
~/.claude/projects/<encoded-project-path>/
├── <session-uuid>.jsonl              # 主会话
└── <session-uuid>/subagents/
    ├── agent-<id>.jsonl              # 结构同主会话
    └── agent-<id>.meta.json          # agent 类型、任务描述
```

目录名 → 真实路径需解码并校验存在性(失效标 orphaned)。CC 会自动清理老会话,不可假设文件持续存在。`~/.claude/history.jsonl` 为 slash 命令历史,与会话无关,不读。

**解析规范(本 provider 的真实难点),pipeline 固定五阶段,禁止合并:**

```
entry 解析 → 分类过滤 → 消息重组 → 树重建 → 展示转换
```

四条规则:

1. **一行 ≠ 一条消息。** 流式输出按 message ID 分散多行,必须按 message ID 分组重组。按行直译会导致 tool_use 为空、消息数缩水至约 1/4,且无报错。
2. **按 UUID 去重。** branching/resume 会将同一 UUID 写入多个文件。计数与 token/cost 统计必须去重后进行。
3. **白名单式 entry 过滤。** 默认全部忽略,白名单加回(user prompt、assistant text、tool_use、tool_result、thinking、元数据)。未知类型忽略 + debug 计数,禁止抛错。
4. **parent-child UUID 构成树。** subagent 分散独立文件,多阶段重建 + 固定 merge 顺序(主会话 → 逐个 subagent),禁止单趟合并。

**Resume:** `claude --resume <session-id>`(先 cd 到项目路径)。
**链信号:** resume-fork 会把父会话历史复制进新文件,被复制行保留原 sessionId——文件头部第一个 ≠ 自身 id 的 sessionId 即链父(发现时从已读的头部字节提取,零额外 IO)。

### 4.2 Codex(v1.1)

**存储:** `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<session-id>.jsonl`,按日期组织;可能存在 `.jsonl.zst` 压缩变体,解压后解析。每行 `{timestamp, type, payload}`:
- `session_meta`(首行): cwd、git.branch、session id——项目分组完全依赖它的 cwd;
- `response_item`: 唯一取信的消息来源。白名单 payload.type: `message`(role user/assistant;developer 忽略;`<environment_context>`/`<user_instructions>` 开头的 user 文本是注入上下文,过滤)、`reasoning`(仅 summary 可显示,encrypted_content 不可用)、`function_call`/`custom_tool_call`(arguments 为 JSON 编码字符串,解码展示)、`*_output`、`web_search_call`;
- `event_msg` 全部忽略——`agent_message`/`user_message` 与 response_item 内容重复,取信会导致消息翻倍;
- 未知 type/payload(如 `ghost_snapshot`)忽略 + 计数。

会话线性,无流式分行、无重复 UUID、无 parent-child 树(CC 四规则不适用,留在 CC provider 内)。消息无内在 id,以 `L<1-based 行号>` 为 uuid,搜索命中据此锚定跳转。
**Resume:** `codex resume <session-id>`(先 cd 到项目路径;常规 resume 追加原文件,不产生新会话)。
**链信号:** session_meta 的 `forked_from_id`(fork)与 `source.subagent.thread_spawn.parent_thread_id`(multi-agent worker 线程,否则与顶层会话无法区分)即链父,随首行一并提取。
**规模注意:** 数万 rollout 文件属正常(本机 1.9 万+),discover 的首行读取必须并行。

### 4.3 Antigravity(v1.2,experimental)

**存储(仅支持的部分):** `~/.gemini/antigravity/brain/<conversation-id>/`,交互历史 JSONL 位于 `.system_generated/logs/`;同目录含 implementation_plan.md、task.md、walkthrough.md 等 artifact。
**不支持:** IDE 侧 `.pb` protobuf 会话与 LanguageServer API 读取(需宿主进程存活,违反 ADR-6)。
**capabilities:** resumable=false,hasArtifacts=true。UI 明确标注 experimental 与覆盖范围。

### 4.4 Cursor(v1.3,experimental)

**存储(macOS,本机 2026-07 实测):** 会话在 Cursor IDE 的 VS Code 派生存储中,SQLite 内嵌 JSON:

```
~/Library/Application Support/Cursor/User/
├── globalStorage/state.vscdb              # cursorDiskKV 表(本机 1.8GB)
│   ├── composerData:<composerId>          # 会话头(本机 416 个)
│   └── bubbleId:<composerId>:<bubbleId>   # 消息正文(本机 3.5 万条)
└── workspaceStorage/<hash>/
    ├── workspace.json                     # folder URI → 真实项目路径
    └── state.vscdb                        # ItemTable 键 composer.composerData
                                           #   → 该 workspace 的 allComposers 元数据列表
```

**发现(两级走查,不碰大库正文):** 枚举 workspaceStorage → `workspace.json` 取项目路径(项目分组派生源,file URI 需解码归一化)→ 该 workspace 小库的 `allComposers` 取 composerId/createdAt/mode。无 workspace 归属的 composer(已删 workspace 的孤儿)忽略 + debug 计数,后续再评估归置。native_id = composerId;RawSession.mtime 取 composer 元数据时间戳(共享单库,文件 mtime 无会话区分度)。

**解析:** globalStorage 点查 `composerData:<id>`。**两代格式必须都支持:** 旧版 `conversation` 数组内嵌 bubble 全文;新版(`_v` ≥ 3)仅 `fullConversationHeadersOnly` 存 bubbleId 序列,正文逐条点查 `bubbleId:<composerId>:<bubbleId>`。bubble `type` 1=user / 2=assistant;字段白名单:text、thinking、tool call/result、tokenCount;未知字段/类型忽略 + 计数。会话线性,无 CC 树、无跨文件重复(CC 四规则不适用,勿套)。

**SQLite 纪律(ADR-7):** `rusqlite` bundled,`mode=ro` 打开 + busy_timeout(Cursor 运行中持续写库,WAL);**禁用 `immutable=1`**(文件在变,会读到损坏页);禁一切写与 WAL checkpoint。discover/parse 全部走主键点查与 `bubbleId:<composerId>:` 前缀范围查询,禁止对 1.8GB 大库全表扫描。

**搜索:** ripgrep 引擎无法作用于 SQLite → `search_roots` 返回空,覆写 `search_compressed`(ADR-3 降级路径):按 scope 内 composer 的 bubble 前缀范围查询遍历匹配,messageUuid 锚点用 bubbleId;composer 级 rayon 并行,每任务独立只读连接(SQLite 并发只读安全)。实测(本机 416 composer / 3.5 万 bubble / 1.8GB 库,release):全库 1.4s,项目内 0.8s,混合全局搜索维持在原 2.2s 地板附近。

**Resume(客户端形态,见 ADR-4 增补):** `launch=GuiApp`,命令 `open -a Cursor <project_path>`;会话级直达无公开 deeplink,UI 展示会话标题辅助用户在 Cursor 聊天历史面板定位。
**capabilities:** resumable=true(客户端级)、has_artifacts=false、project_native=false。
**不支持(当前):** cursor-agent CLI 会话——本机无样本、格式未验证,`~/.cursor/projects/<encoded-path>/` 仅见 terminals/rules/mcps 无会话正文;待有真实样本再评估,不阻塞 IDE 侧。

### 4.5 Qoder(v1.4)

**存储:** `~/.qoder/projects/<encoded-project-path>/`,布局神似 CC 但契约更友好——**一个顶层会话 = 一对文件**:

```
<session-id>-session.json    # 元数据:title、working_dir(权威 cwd,无需目录名解码)、
                             #   parent_session_id(链父)、prompt/completion_tokens、cost、时间戳
<session-id>.jsonl           # 线性正文,每行一条消息
```

同目录的其他内容不是会话:无 `-session.json` 配对的 `toolu_*.jsonl`(tool 中间产物)与 `<session-id>/` checkpoint 快照目录,发现与搜索均排除。**注意:** 带 `-session.json` 配对的 `toolu_*` 是 subagent Task 会话(title "Task: …"、parent_session_id 指向父)——正常枚举,经 parent 链机制自动折叠到父会话下,无需特判。

**解析(线性,CC 四规则不适用):** 每行 `{id, session_id, role, parts[], created_at(ms), is_meta}`。`is_meta` 行过滤 + 计数;role 白名单 user/assistant/tool(tool 行承载 tool_result,按 assistant 侧展示),未知 role 忽略 + 计数;parts 白名单:`text`、`tool_call`(input 为 JSON 编码字符串,解码展示)、`tool_result`(content 可为字符串或嵌套块,收集字符串叶)、`finish`(已知终止标记,不展示),未知 type 忽略 + 计数。消息 uuid = 行 `id`。token 统计直接取 session.json(无需 CC 式去重累加),extensions.usage 与 CC 同 shape。
**搜索:** 纯 JSONL 直接进 ripgrep 引擎;项目 scope 按目录首个 session.json 的 working_dir 收窄(同 CC 模式);resolve_hit 要求 `.jsonl` + 存在配对元数据,天然排除 checkpoint 与孤儿 toolu 文件。
**Resume:** `qodercli -r <session-id>`(先 cd 到项目路径;Terminal 形态,同 CC/Codex)。
**链信号:** session.json 的 `parent_session_id` 即链父,发现时零额外 IO。
**capabilities:** resumable=true、file_based_only=true、has_artifacts=false、project_native=true。

### 4.x Trae(调研结论:不做)

2026-07 实测(Trae 2.x):AI 会话不落本地盘——`state.vscdb` 仅存输入历史(`icube-ai-agent-storage-input-history`)与模型配置,IndexedDB 仅 20KB 无会话内容,会话正文存字节云端(账号同步)。无本地数据可读,连降级读取的对象都不存在,ADR-6 直接不适用。若 Trae 未来引入本地会话存储再评估。

## 5. Core 统一模型与服务

```rust
struct Project        { id, real_path, exists, session_count, last_active, last_prompt?, providers: BTreeSet<String> }
struct RawSession     { provider_id, native_id, file_path, project_path, mtime, file_size, parent_native_id? } // 可序列化:壳层持有分页句柄;parent 承载 resume/fork 链
struct SessionSummary { id, provider_id, native_id, project_path, title, mtime, message_count, git_branch?, duration }
struct Session        { summary, messages: Vec<Message>, extensions: Map<String, Value> }
struct Message        { uuid, parent_uuid?, role, blocks: Vec<ContentBlock>, timestamp }
enum ContentBlock     { Text{text}, ToolUse{name, input}, ToolResult{summary, truncated}, Thinking{text} }

trait SearchEngine { fn search(&self, query: &str, scope: &Scope) -> Result<Vec<SearchHit>> } // grep crates 实现
trait Resumer      { fn resume(&self, spec: &ResumeSpec) -> Result<()> }                      // CLI/GUI 各一实现
```

序列化统一 serde camelCase,即 `--json` 与 Tauri IPC 共用同一 schema。

`SearchHit` 含 providerId、sessionId、messageUuid、命中片段与上下文,支撑跳转定位。outline/page 是详情页懒加载基础,大文件禁止一次性载入 UI。

## 6. CLI 规格

```
vibetrail projects [--json]
vibetrail sessions <project> [-n 20] [--provider <id>] [--json]
vibetrail search <query> [-p <project>] [--provider <id>] [--json]
vibetrail show <session-id> [--outline|--full] [--json]     # 默认 outline
vibetrail resume <session-id>
vibetrail open [<project>]                                  # 拉起 GUI
vibetrail config [--json]                                   # 生效配置(§12)
```

- `--json` schema 在 Core 定义(Codable),GUI/CLI 共用;这是未来 MCP server 的接口雏形。
- 退出码:0 成功 / 1 参数错误 / 2 数据错误 / 3 resume 前置校验失败 / 4 provider 不支持该操作。
- session-id 跨 provider 唯一化:内部键为 `<providerId>:<native-id>`,CLI 接受 native-id 唯一前缀。

## 7. GUI 结构

Tauri v2:Rust 薄壳注册 commands(`list_projects` / `list_sessions` / `get_session` / `search` / `resume`),全部直调 Core;前端为纯静态三栏布局,经 IPC invoke 取数。

```
三栏布局(ui/)
├── Sidebar: 项目列表(F1,provider 徽标聚合,可隐藏项目)
├── Middle:  会话列表(F2)+ 顶栏搜索(F4,覆盖层结果)
└── Detail:  时间线(F3)+ Resume 按钮(F5,按 capability 显隐)
```

tool call / result / thinking 折叠为单行摘要,点击展开;超长 tool result 截断 + "展开全文";搜索结果点击 → 详情页 `scrollTo(messageUuid)`。

**项目隐藏:** 纯 GUI 展示偏好——行内 ⊘ 隐藏、列表底部 "N hidden projects" 折叠区内 ↩ 取消隐藏。条目存 `config.json` 的 `hiddenProjects`,可为字面路径(normalized real path)或 glob 通配符(`*` 段内、`**` 跨段、`?` 单字符;不含 `/` 的条目按项目名匹配,gitignore 式),通配符条目经设置面板 Workspace 区输入。行内 ↩ 移除命中该项目的全部条目,连带移除通配符时 toast 告知;设置面板按条目逐条管理并显示每个 pattern 的命中数。Core 发现、搜索与 CLI 输出不受影响,不引入新的存储或索引。

**设置面板:** 主入口为原生菜单栏 App 菜单的 "Settings…"(⌘,,macOS 标准位置,About 之后;在 Tauri 默认菜单上插入,菜单事件经 `open-settings` 事件通知前端),侧栏 ⚙ 为窗口内镜像入口。分组与 schema 见 §12;对应 commands `settings_info`(生效配置报告,与 CLI `config --json` 同 schema)与 `reveal_config`(Finder 定位配置文件)。

**交互响应性(2026-07,"跟手"修复),两条纪律:**
1. **主线程零阻塞。** Tauri 同步 command 在主线程执行,任何 store 触达(活读,本机规模秒级)都会冻住渲染、hover 与点击——这是卡顿的根源。所有做 IO/spawn 子进程的 command 一律 `async fn` + `spawn_blocking`,禁止新增同步耗时 command。
2. **壳层 SWR 缓存。** 点击项目立即渲染上次结果、后台活读校验后差异替换(handles 按项目路径、summary 按 `provider:id@mtime` 键控,mtime 变化即自然失效);启动时 `list_all_handles` 一次全店发现预热缓存——项目总览本就付过这次读,此前把 handles 丢弃了。缓存只活在窗口内存,数据源设置变更即清空,活读仍是唯一真相源:ADR-2 的"无索引、无缓存"约束 Core 与持久层,不约束展示层内存。
   另:时间线渲染块 200→80 条/帧(整块 markdown 解析会卡帧);慢加载(>150ms)才显示 loading 态,快路径不闪烁;项目/会话/搜索均有 generation 竞态防护,后发请求胜出。

**国际化(2026-07):** 前端手写 i18n(`ui/i18n.js`),en/zh 双字典 + `t(key, params)`,静态文本经 `data-i18n(-title/-placeholder)` 属性填充——零依赖零构建链(ADR-1 前端纪律不破)。语言偏好存 config.json `language`("auto"|"en"|"zh",auto 由前端按 `navigator.language` 解析);设置面板切换后保存并 reload(低频操作,reload 比全视图原地重渲染简单可靠)。原生菜单 "Settings…" 仅在显式选 zh 时本地化(原生层无廉价 locale 探测,不为此引插件)。**边界:** 后端(Rust)产生的错误消息与 resume 提示保持英文,不跨 IPC 做消息本地化;provider 名称与键名等专有名词不翻译。

**徽标配色:** 五个已知 provider 各占一个可辨识色相(CC 砖红/CX 绿/AG 蓝/CU 紫/QD 琥珀),未知 provider 回落灰色;全部中性色,不使用厂商品牌色(开源卫生)。

**自更新(2026-07,tauri-plugin-updater):** manifest 为 GitHub release 的 `latest.json`(endpoint `releases/latest/download`),更新包 minisign 验签(公钥在 tauri.conf.json)+ Apple 签名公证双重保障。交互纪律:**永不静默安装**——启动 5s 后后台静默检查(离线失败静默吞掉),发现新版展示持久横幅,用户点击才下载安装并重启;设置面板有手动检查与当前版本号。签名私钥在 GitHub secrets(`TAURI_SIGNING_PRIVATE_KEY`)与发布者本机 `~/.tauri/vibetrail-updater.key`——**丢失即所有已装 app 永远无法再验证更新,必须备份**。安全边界澄清:更新检查是 app 唯一的主动出站请求(GitHub,仅版本查询);自更新替换应用包自身是该功能的本质,"config.json 是唯一写入文件"的约定指用户数据面,不含 app 自身的原子替换。CI 侧 tauri-action 注入签名 env 并以 `includeUpdaterJson` 上传 manifest。

**图标纪律:** 图标源图必须遵守 Apple 图标网格——内容(squircle)占画布 824/1024(80.5%),四边各留 ~9.8% 透明边距,圆角半径 22.5%。满幅填充的图标在 macOS(尤其 15+)的 Dock 里会比系统图标大一圈。`bundle.icon` 与运行时 Dock 图标共用 `icons/icon.png`,换图时先量 bbox 再提交。

## 8. 性能预算

| 场景 | 预算(数百 session/数百 MB 规模) | 手段 | 实测(本机 2 万 session / 3.4GB,release) |
|------|------|------|------|
| 冷启动 → 项目总览 | < 500ms | discover 只读目录元数据 + 首行/首块,不全文解析;rayon 并行 | 1.4s(规模超预算基准 50×) |
| 会话列表 | < 300ms | 发现与摘要拆分:句柄一次取回,摘要按 50/页并行 parse | 首页 ~0.7s(717 会话项目) |
| 全局搜索 | < 1s | grep 引擎并行 + provider 并行 + 500 命中熔断;项目内搜索收窄到文件级 | 常见词 0.1s;稀有词全库 2.2s(= rg 地板);项目内 0.5–2.1s |
| 打开大会话 | 首屏 < 500ms | 时间线 200 条/块懒渲染;tool result 截断 2000 字符 + 按需取全文 | 打开会话 0.06s |

单次操作成本纪律:打开一个会话禁止全店 discover(`find` 短路)、禁止全量 parse(resume 走元数据、can_resume 零 IO)。

## 9. 测试策略

- **per-provider fixture 对拍:** 每个 provider 收集真实样本(CC:流式多行、branching 重复 UUID、subagent、未知类型;Codex:zst、session_meta 变体;Cursor:从真实 state.vscdb 抽样脱敏构造 mini vscdb,覆盖新旧两代 composerData 与孤儿 composer),断言消息数、tool call 数与人工核对值一致。这是格式漂移的回归防线。
- CC 解析层单测覆盖四条规则各自的反例输出。
- `--json` schema 快照测试。
- GUI 手测,v1 不做 UI 自动化。

## 10. 里程碑

| 阶段 | 交付 | 验收 | 状态 |
|------|------|------|------|
| M1 | Core 统一模型 + Provider 协议 + CC provider 解析,CLI `projects/sessions/show` | 本机 `~/.claude` 对拍全部正确 | ✅ |
| M2 | CLI `search/resume`,`--json` 全覆盖 | CLI 完整可用,自用替代 `/resume` | ✅ |
| M3 | GUI 三视图 + 搜索 + Resume | PRD P0 闭环,开源发布 v1 | ✅ |
| M4 | Codex provider | 抽象验证通过(无需改协议或改动极小),发布 v1.1 | ✅(trait 增加 line_number/search_compressed 两处,详见 §3) |
| M5 | Antigravity provider(experimental)、P1 项 | v1.2 | ✅(P1 token 统计不含 cost 换算——定价表随模型漂移,只做 token) |
| M6 | Cursor provider(experimental)+ ResumeSpec 客户端形态(ADR-7、§4.4) | fixture 对拍 + 真机 resume 手测,发布 v1.3 | 代码完成,fixture 对拍与真实库 smoke 通过;GUI/resume 手测待用户验证 |
| M7 | Qoder provider(§4.5);Trae 调研结论:云端存储无本地数据,不做 | fixture 对拍 + 真机 resume 手测,发布 v1.4 | 代码完成,fixture 对拍与真实库 smoke 通过;resume 手测待用户验证 |

M1/M2 先行:CLI 是 Core 的测试驱动器,GUI 只是换皮。

## 11. 开发约定(供 Claude Code 遵守)

- Core 不依赖 tauri/任何 GUI 或终端 UI crate;壳层不直接读任何 agent 存储目录。
- Provider 之间零依赖;provider 特有逻辑(CC 四规则、Codex zst)禁止泄漏到 Core 通用层。
- 对所有 agent 存储目录严格只读。
- 未知 entry/格式变体:忽略 + 计数,禁止抛错中断。
- 所有路径操作先校验存在性;错误信息含具体文件路径。

## 12. 配置与设置

**原则:配置即文件。** `~/.config/vibetrail/config.json` 是设置的唯一事实源,也仍是 VibeTrail 唯一写入的文件(ADR-4 不变):纯 JSON、可手编、可进 dotfiles。GUI 设置面板只是它的薄编辑器;保存时本版本不认识的字段原样保留(手编内容与更新版本的字段不被打掉)。缺失或损坏的配置一律降级为默认值——配置错误绝不允许 brick 发现流程。

**schema(App 壳持有全量,Core 只读 `providers` 切片):**

```json
{
  "terminal": "terminal | iterm2 | ghostty | warp",
  "hiddenProjects": ["/abs/normalized/path", "**/scratch/**", "tmp-*"],
  "providers": {
    "claude-code": { "enabled": true },
    "codex":       { "enabled": true, "root": "~/alt/codex/sessions" },
    "antigravity": { "enabled": false }
  }
}
```

**发现配置下沉 Core。** provider 启用与存储根覆盖决定 `SessionStore` 的构成,属发现逻辑,实现在 `vibetrail-core::config`(静态 provider 注册表 + `store_from_config`);CLI 与 GUI 均经 `default_store()` 建店,同一份配置在两个壳行为一致。`enabled=false` 的 provider 不参与发现、搜索与 resume;`root` 支持 `~`,空串等价缺省;省略的 provider 取 `{enabled: true, root: 默认}`。写入仍只发生在 GUI 壳,Core 对配置文件只读。

**设置面板按工程工具维度分组(而非杂项开关堆):**

| 维度 | 内容 | 类比 |
|------|------|------|
| Data sources | 各 provider 启用开关、存储根覆盖(占位符即默认路径)、路径有效性状态(found / missing / disabled) | IDE 的 SDK/toolchain 配置 |
| Resume | 终端选择(含 Warp 降级说明) | 运行/工作流配置 |
| Workspace | 隐藏项目集中管理(查看、Unhide) | 工作区视图配置 |
| 配置文件 | 底部常驻:config.json 路径 + Reveal in Finder | "配置即文件"的出口 |

**可检查性。** `vibetrail config [--json]` 输出生效配置:配置文件路径,以及每个 provider 的 enabled、生效 root、默认 root、是否自定义、路径是否存在。`--json` schema(`ConfigReport` / `ProviderStatus`)在 Core 定义、有快照测试,与 GUI `settings_info` command 共用——设置没有第二套数据通道。
