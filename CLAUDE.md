# VibeTrail

多 agent 会话浏览与恢复工具(Tauri App + CLI,Rust workspace,开源)。浏览、搜索 Claude Code / Codex / Antigravity 的历史会话,一键 resume。

## 文档

- 需求见 `PRD.md`,设计见 `TECH_SPEC.md`。冲突时以 TECH_SPEC 为准并更新文档。

## 架构铁律

- 一份 Core(`vibetrail-core`)+ 两个薄壳(CLI `vibetrail-cli` / Tauri App)+ N 个 Provider。业务逻辑只许在 Core,壳层发现逻辑必须下沉。
- Core 不依赖 tauri/任何 GUI crate;壳层不直接碰任何 agent 存储目录。Tauri 前端保持纯静态 HTML/CSS/JS,不引入 Node 构建链。
- Provider 之间零依赖;provider 特有逻辑禁止泄漏到 Core 通用层。项目分组是从 cwd 派生的,不是存储属性。
- 无数据库、无索引、无常驻进程、无 FS watcher。搜索 link ripgrep 引擎 crates(grep-searcher/grep-regex),无外部 rg 依赖。
- Provider 现状:ClaudeCode(v1)/Codex(v1.1)/Antigravity(v1.2, experimental)均已实现;Provider trait 与 capabilities 见 TECH_SPEC 第 3 节。新 provider 准入守 ADR-6(纯文件读取)。

## Claude Code Provider 解析四规则(违反即 bug)

1. 按 message ID 跨行分组重组消息,禁止按行直译。
2. 按 UUID 去重后再计数/统计。
3. entry 类型白名单式处理,未知类型忽略 + debug 计数,禁止抛错。
4. parent-child 为树;subagent 文件按固定顺序 merge,禁止单趟合并。

## 安全约定

- 对 `~/.claude`、`~/.codex`、`~/.gemini` 等 agent 目录严格只读。
- resume 前必须校验项目真实路径存在。

## 工作流

- 改动任何 provider 解析必须先跑对应 fixture 对拍测试。
- 新增 CLI 输出字段须同步更新 `--json` schema 快照测试。
- 提交信息英文,conventional commits。
- 开源卫生:不引入任何 agent 厂商品牌资源;README 保持 unofficial 声明。
