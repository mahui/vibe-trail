# PRD — VibeTrail

**版本:** v0.4
**日期:** 2026-07-02
**状态:** v1.2 已交付(P0+P1 全量),持续迭代
**形态:** 开源项目(macOS App(Tauri)+ CLI,Rust workspace)

---

## 1. 一句话定位

VibeTrail — 浏览、搜索所有 coding agent 的历史会话,一键回到工作现场。
Tagline: *Session browser & resume for coding agents (Claude Code, Codex, Antigravity, …)*

## 2. 背景与问题

Coding agent(Claude Code、Codex、Antigravity 等)将会话历史散落在本地各自的私有目录与格式中:

- 各家自带的 resume 入口只能看到最近少量会话,无跨项目视图,更无跨 agent 视图;
- 无任何跨会话搜索能力,"上周在哪个 session 里改过 nginx 配置"无法回答;
- 多 agent 并用已是常态,但没有任何工具提供统一入口;
- 现有第三方工具(lm-assist、claude-vault、各类 viewer)要么重(常驻 server + 向量索引),要么是单一 agent 的只读"考古工具",都没有打通"找到 → 继续工作"的闭环。

## 3. 差异化

1. **闭环:浏览 + 搜索 + 一键 resume。** 竞品止步于"看",VibeTrail 的终点是"回到终端继续干活"。
2. **多 agent 统一入口。** Provider 抽象,一个界面看所有 agent 的工作轨迹。
3. **轻。** 无数据库、无索引、无常驻进程、无 FS watcher,活读文件;搜索内联 ripgrep 引擎 crates。
4. 双入口:GUI(Tauri)+ CLI。

与 VibeSpace 构成产品家族:Space 是 agent 工作空间的安全网,Trail 是 agent 工作历史的回溯线。

## 4. 目标用户

macOS 上的 coding agent 重度用户(每天多个 session、多项目、可能多 agent 并行)。

## 5. Provider 路线图

| 版本 | Provider | 说明 | 状态 |
|------|----------|------|------|
| v1 | Claude Code | 全能力(浏览/搜索/resume) | ✅ |
| v1.1 | Codex | 纯文件读取(含 .zst 解压),验证 Provider 抽象 | ✅ |
| v1.2 | Antigravity | Experimental,仅读 brain/ 下 JSONL 部分;.pb/LanguageServer 依赖宿主进程,不做 | ✅ |
| v1.3 | Cursor | Experimental,只读解析 IDE 的 state.vscdb(SQLite,见 TECH_SPEC ADR-7);resume = 打开 Cursor 客户端到项目,会话级直达待官方 deeplink;cursor-agent CLI 待样本 | 代码完成,待手测发布 |
| v1.4 | Qoder | 全能力(浏览/搜索/resume `qodercli -r`),纯文件读取,存储布局类 CC(见 TECH_SPEC §4.5) | 代码完成,待手测发布 |
| — | Trae | 不做:AI 会话存云端,本地仅输入历史,无数据可读(TECH_SPEC §4.x 调研结论,2026-07) | 关闭 |
| 后续 | 社区贡献 | Provider 协议开放,接受 PR(见 CONTRIBUTING.md) | 开放 |

原则:能纯文件读取就支持;需要宿主进程活着或逆向私有格式的,降级或不做,不为最弱的 provider 污染架构承诺。

## 6. 功能需求

### P0(v1,仅 Claude Code provider)

| # | 功能 | 说明 |
|---|------|------|
| F1 | 项目总览 | 跨 provider 聚合,按归一化 cwd 分组:真实路径、session 数、最近活跃、最近 prompt 摘要、provider 标识。路径已失效的标灰。 |
| F2 | 会话列表 | 单项目内按 mtime 倒序:标题(首条 user prompt)、时间、消息数、git branch、耗时、provider 图标。 |
| F3 | 会话详情 | 消息时间线:prompt/回复正常展示;tool call、tool result、thinking 默认折叠。大会话懒加载分页。 |
| F4 | 全局搜索 | 顶栏常驻,跨全部 provider 与项目全文搜索;可限定项目;结果按 session 聚合、命中高亮;点击跳转并定位到命中消息。 |
| F5 | 一键 Resume | 仅对 capability 声明可 resume 的 provider 显示。GUI:打开配置的终端执行 resume 命令;CLI:直接 exec。resume 前校验项目路径存在。 |
| F6 | CLI | `vibetrail projects/sessions/search/show/resume/open`,查询类支持 `--json`。 |

### P1(已交付)

- ✅ subagent 会话树状展示(CC provider 特有;Codex 的 multi-agent worker 线程同样归属父会话)
- ✅ token 统计(UUID 去重后累加;cost 换算刻意不做——定价表随模型漂移,token 不会)
- ✅ 终端选择配置(Terminal.app / iTerm2 / Ghostty 直接执行;Warp 无可脚本化执行面,降级为定位目录 + 命令入剪贴板)

### 交付后增量(用户反馈驱动)

- resume/fork 链聚合:续会话/fork/subagent 线程折叠到根会话下(GUI ⑂ 徽标,CLI ↳ 标记)
- 会话正文 markdown 渲染(DOMPurify 消毒;链接跳系统浏览器)
- session id 展示与一键复制(列表短 id 芯片 + 详情完整 id 独立行)
- 超长 tool 输出:预览 2000 字符 + 按需加载全文
- 会话列表持续加载(50/页无限滚动);搜索结果点击不再关闭结果列表
- 设置功能,按工程工具维度组织(TECH_SPEC §12):数据源(provider 开关 + 存储根覆盖 + 路径状态)/ Resume 终端 / 隐藏项目管理 / 配置文件出口(Reveal in Finder);发现配置下沉 Core,CLI 与 GUI 行为一致;新增 `vibetrail config [--json]` 检查生效配置
- GUI 交互响应性("跟手")修复:Tauri command 全部移出主线程 + 壳层 SWR 缓存 + 启动预热,点击即时反馈(TECH_SPEC §7)
- App 界面国际化:en/zh,设置面板切换(Auto/English/中文),偏好存 config.json;CLI 保持英文(TECH_SPEC §7)
- 侧栏项目筛选:项目名实时搜索 + agent 徽标筛选(减法模型:默认全亮,亮=显示/实心色块,灭=排除/空心描边,视觉状态与结果始终一致;熄灭最后一个自动重置全亮);纯展示层,会话级不持久化
- App 自更新:启动后台检查 GitHub Releases,新版横幅提示、点击安装重启,永不静默;设置面板手动检查 + 版本号;minisign 验签(TECH_SPEC §7)
- 信息架构修正:搜索范围显性化(placeholder 随选中项目联动 + 结果页 sticky 范围头部条:命中数/范围/一键搜全部/退出);侧栏置顶跨项目 Recent 视图(最新会话,行内带所属项目);会话 meta 行时间前置加亮;Resume 按钮 tooltip 按 provider 预告行为;筛选框与搜索框视觉分层;设置 Language 归入"界面"组

### 非目标(v1 明确不做)

- 语义搜索 / embedding
- 任何索引 / 数据库 / 缓存层
- FS watcher 或常驻后台进程
- MCP server 入口(core 已预留 `--json` schema,后续加壳)
- 会话归档 / 防删除
- Antigravity 的 .pb / LanguageServer API 读取
- Windows / Linux

### Future(记录,不排期)

- Agent 工作产物(artifact)浏览:Antigravity 的 plan/task/walkthrough 提示了"transcript 浏览器 → 工作产物浏览器"的升级方向
- 本地 embedding 语义搜索
- MCP server:让新 agent session 程序化查询历史语料(自改进循环)

## 7. 用户故事

1. 我记得三周前某个 session 里定位过证书路径问题,搜 "certificate path",5 秒找到,点 Resume 回到当时上下文继续。
2. 我并行维护 5 个项目、混用 CC 和 Codex,打开 app 一眼看到每个项目最近谁在干什么。
3. 终端里 `vibetrail search "race condition" --json | jq` 把结果喂给脚本。

## 8. 开源策略

- **License:** Apache-2.0(已定,见 TECH_SPEC ADR-5)。
- **Repo:** `vibetrail`,description 与 topics 覆盖 `claude-code` / `codex` / `antigravity` / `session` / `resume` / `agent` 等搜索词——开源项目可发现性 > 品牌性,关键词全部放 metadata,不占名字。
- **商标卫生:** README 显著位置声明 unofficial / not affiliated;产品名与图标不使用任何 agent 厂商品牌元素。
- **分发:** CLI 走 Homebrew(复用现有 tap);App 提供 GitHub Releases 签名 dmg。
- **定位:** build-in-public 名片 + Vibe 产品家族引流资产。若后续引入付费能力(语义搜索等),届时再评估 open core 拆分,Core+壳分层架构已为此预留边界。

## 9. 成功指标

- 冷启动到项目总览 < 500ms(数百 session 规模)
- 全局搜索 < 1s
- 自用一周内替代各家原生 resume 列表
- 开源侧:首月 GitHub star 与 issue 中出现非本人提交的 provider 适配请求(验证多 agent 定位)

## 10. 风险

| 风险 | 应对 |
|------|------|
| 各家会话格式无文档且随版本漂移 | Provider 隔离 + fixture 对拍;未知 entry 容忍策略 |
| Provider 泥潭:每家格式都是流沙 | v1 只 ship CC 一个 provider,抽象先建但不铺 adapter |
| 官方补齐会话管理 | 差异化押在跨 agent 统一 + resume 闭环 + CLI |
| "Vibe" 命名潮流退坡 | 与 VibeSpace 家族绑定,品牌资产共担 |
