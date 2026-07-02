# 技术设计 — VibeTrail

**版本:** v0.2
**日期:** 2026-07-02
**配套文档:** PRD.md

---

## 1. 总体架构

一份 Core,两个薄壳,N 个 Provider。所有业务逻辑在 Core;GUI 与 CLI 仅做调用与呈现;每个 agent 生态的差异被 Provider 协议隔离在各自实现内。

```
vibetrail/                        # Swift Package workspace
├── Sources/
│   ├── VibeTrailCore/            # 统一模型、Provider 协议、搜索、resume 编排
│   │   └── Providers/
│   │       ├── ClaudeCode/       # v1
│   │       ├── Codex/            # v1.1
│   │       └── Antigravity/      # v1.2, experimental
│   ├── vibetrail/                # CLI target(swift-argument-parser)
│   └── VibeTrailApp/             # SwiftUI macOS app
└── Tests/
    └── VibeTrailCoreTests/       # 各 provider fixture 对拍
```

无 HTTP server、无数据库、无常驻进程、无 FS watcher。

## 2. ADR 摘要

### ADR-1:技术栈 Swift/SwiftUI(而非 Rust/Tauri)

**状态:** 已接受
**理由:** 原生质感,与 VibeSpace/Pier 产品线同栈同气质;menu bar / 后续能力扩展均为熟路。
**代价:** 放弃跨平台;搜索无法 link ripgrep crate(改 shell out,ADR-3)。
**备选:** Rust + Tauri。开源后若社区强烈要求 Linux 再评估;Core 接口保持可移植语义。

### ADR-2:活读文件,不建索引

**状态:** 已接受
**理由:** 数百 session / 数百 MB 规模下目录扫描 + rg 均在秒级内;无索引则无一致性问题、无后台进程。
**升级路径:** `SearchEngine` 协议后加 FTS5 缓存实现,接口不变。

### ADR-3:搜索 shell out ripgrep

**状态:** 已接受
**实现:** 优先 PATH 中的 `rg`;不存在则用 app bundle 内置 rg(universal binary)。Homebrew formula 声明 `ripgrep` 依赖。压缩会话(Codex .zst)搜索经 provider 解压后喂给 rg 或降级为 provider 内搜索。

### ADR-4:Resume 实现

**状态:** 已接受
**CLI:** 校验路径后 `chdir` 到项目路径并 `exec` provider 给出的 resume 命令。
**GUI:** AppleScript 拉起用户配置的终端执行同命令。v1 支持一种终端,终端适配层为协议,后续扩展。
**安全:** 无网络监听面;AppleScript 需 Automation 权限,首次触发引导授权。

### ADR-5:License 与开源结构

**状态:** 暂定(唯一待拍板项)
**暂定:** MIT,单 repo 全开源。
**备选:** open core(Core+CLI MIT,App 闭源收费)。当前判断:先全开源换社区与 star,付费能力(语义搜索等)出现时再评估拆分;Core/壳边界从第一天保持干净,保留拆分自由度。

### ADR-6:Provider 抽象与准入原则

**状态:** 已接受
**原则:** 只接纳纯文件读取可覆盖的能力。需要宿主进程存活(如 Antigravity LanguageServer API)或逆向无 schema 私有格式(.pb)的能力一律不做或降级,不为最弱 provider 污染"零依赖活读"的架构承诺。
**纪律:** 抽象第一天建立,但 v1 只 ship Claude Code 一个实现;Codex(v1.1)的用途是验证抽象切分是否正确——单实现的抽象是猜测,两个实现才算数。

## 3. Provider 协议

```swift
protocol Provider {
    var id: String { get }                // "claude-code" / "codex" / "antigravity"
    var capabilities: ProviderCapabilities { get }
    func discover() throws -> [RawSession]                 // 枚举存储,轻量(元数据级)
    func parse(_ raw: RawSession) throws -> Session         // 归一化到统一模型
    func outline(_ raw: RawSession) throws -> [MessageStub] // 懒加载
    func page(_ raw: RawSession, offset: Int, limit: Int) throws -> [Message]
    func resumeSpec(_ s: SessionSummary) -> ResumeSpec?     // nil = 不可 resume
}

struct ProviderCapabilities {
    let resumable: Bool        // CC ✓ / Codex ✓ / AGY ✗
    let fileBasedOnly: Bool    // CC ✓ / Codex ✓ / AGY 部分
    let hasArtifacts: Bool     // AGY ✓(plan/task/walkthrough)
    let projectNative: Bool    // CC ✓ / Codex ✗(按日期) / AGY ✗(按 conversation)
}
```

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

### 4.2 Codex(v1.1)

**存储:** `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<session-id>.jsonl`,按日期组织;首行为 `session_meta` 元数据块(含 cwd);老会话压缩为 `.jsonl.zst`,需解压后解析。
**Resume:** Codex 自有 resume 机制,实现时核对当期 CLI 参数。
**注意:** 项目分组完全依赖 session_meta 的 cwd 提取。

### 4.3 Antigravity(v1.2,experimental)

**存储(仅支持的部分):** `~/.gemini/antigravity/brain/<conversation-id>/`,交互历史 JSONL 位于 `.system_generated/logs/`;同目录含 implementation_plan.md、task.md、walkthrough.md 等 artifact。
**不支持:** IDE 侧 `.pb` protobuf 会话与 LanguageServer API 读取(需宿主进程存活,违反 ADR-6)。
**capabilities:** resumable=false,hasArtifacts=true。UI 明确标注 experimental 与覆盖范围。

## 5. Core 统一模型与服务

```swift
struct Project        { id, realPath, exists, sessionCount, lastActive, providers: Set<String> }
struct SessionSummary { id, providerId, projectPath, title, mtime, messageCount, gitBranch?, duration }
struct Session        { summary, messages: [Message], extensions }
struct Message        { uuid, parentUuid?, role, blocks: [ContentBlock], timestamp }
enum ContentBlock     { text, toolUse(name, input), toolResult(summary, truncated), thinking }

protocol SearchEngine { func search(query:, scope: Scope) -> [SearchHit] }  // rg 实现
protocol Resumer      { func resume(spec: ResumeSpec) throws }              // CLI/GUI 各一实现
```

`SearchHit` 含 providerId、sessionId、messageUuid、命中片段与上下文,支撑跳转定位。outline/page 是详情页懒加载基础,大文件禁止一次性载入 UI。

## 6. CLI 规格

```
vibetrail projects [--json]
vibetrail sessions <project> [-n 20] [--provider <id>] [--json]
vibetrail search <query> [-p <project>] [--provider <id>] [--json]
vibetrail show <session-id> [--outline|--full] [--json]     # 默认 outline
vibetrail resume <session-id>
vibetrail open [<project>]                                  # 拉起 GUI
```

- `--json` schema 在 Core 定义(Codable),GUI/CLI 共用;这是未来 MCP server 的接口雏形。
- 退出码:0 成功 / 1 参数错误 / 2 数据错误 / 3 resume 前置校验失败 / 4 provider 不支持该操作。
- session-id 跨 provider 唯一化:内部键为 `<providerId>:<native-id>`,CLI 接受 native-id 唯一前缀。

## 7. GUI 结构

```
NavigationSplitView
├── Sidebar: 项目列表(F1,provider 徽标聚合)
├── Content: 会话列表(F2)+ 顶栏搜索(F4,覆盖层结果)
└── Detail:  时间线(F3)+ Resume 按钮(F5,按 capability 显隐)
```

tool call / result / thinking 折叠为单行摘要,点击展开;超长 tool result 截断 + "展开全文";搜索结果点击 → 详情页 `scrollTo(messageUuid)`。

## 8. 性能预算

| 场景 | 预算 | 手段 |
|------|------|------|
| 冷启动 → 项目总览 | < 500ms | discover 只读目录元数据 + 首行/首块,不全文解析 |
| 会话列表 | < 300ms | mtime 排序 |
| 全局搜索(数百 MB) | < 1s | rg;zst 部分允许放宽或延迟 |
| 打开 5MB 会话 | 首屏 < 500ms | outline 先行 + 分页 |

## 9. 测试策略

- **per-provider fixture 对拍:** 每个 provider 收集真实样本(CC:流式多行、branching 重复 UUID、subagent、未知类型;Codex:zst、session_meta 变体),断言消息数、tool call 数与人工核对值一致。这是格式漂移的回归防线。
- CC 解析层单测覆盖四条规则各自的反例输出。
- `--json` schema 快照测试。
- GUI 手测,v1 不做 UI 自动化。

## 10. 里程碑

| 阶段 | 交付 | 验收 |
|------|------|------|
| M1 | Core 统一模型 + Provider 协议 + CC provider 解析,CLI `projects/sessions/show` | 本机 `~/.claude` 对拍全部正确 |
| M2 | CLI `search/resume`,`--json` 全覆盖 | CLI 完整可用,自用替代 `/resume` |
| M3 | GUI 三视图 + 搜索 + Resume | PRD P0 闭环,开源发布 v1 |
| M4 | Codex provider | 抽象验证通过(无需改协议或改动极小),发布 v1.1 |
| M5 | Antigravity provider(experimental)、P1 项 | v1.2 |

M1/M2 先行:CLI 是 Core 的测试驱动器,GUI 只是换皮。

## 11. 开发约定(供 Claude Code 遵守)

- Core 不 import AppKit/SwiftUI;壳层不直接读任何 agent 存储目录。
- Provider 之间零依赖;provider 特有逻辑(CC 四规则、Codex zst)禁止泄漏到 Core 通用层。
- 对所有 agent 存储目录严格只读。
- 未知 entry/格式变体:忽略 + 计数,禁止抛错中断。
- 所有路径操作先校验存在性;错误信息含具体文件路径。
