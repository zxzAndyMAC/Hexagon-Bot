<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="Hexagon-Bot logo" />
</p>

<h1 align="center">Hexagon-Bot</h1>

<p align="center">
  <a href="README.md" lang="en">English</a> · <strong lang="zh-CN">简体中文</strong>
</p>

<p align="center">
  <strong>在本机组建 AI 软件开发团队，把需求推进成可运行的代码。</strong>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/zxzAndyMAC/Hexagon-Bot/actions/workflows/ci.yml"><img src="https://github.com/zxzAndyMAC/Hexagon-Bot/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/status-early%20development-e8a33d" alt="Status: early development" /></a>
</p>

<p align="center">
  <a href="#核心能力">核心能力</a> ·
  <a href="#快速开始">快速开始</a> ·
  <a href="#开发与贡献">开发与贡献</a> ·
  <a href="#许可证">许可证</a> ·
  <a href="#致谢与参考">致谢与参考</a>
</p>

Hexagon-Bot 是面向软件开发的本地桌面工作台。你作为负责人，选择角色、绑定模型、指定项目目录；Agent 按流程分工，通过规格、设计、代码和测试记录交接工作。你可以随时调整方向、处理待决事项，并做最终验收。

**当前处于早期开发阶段，以下以源码启动为准。** 桌面完整质量检查目前在 macOS CI 上运行；Linux CI 专项检查终端无法保证所需读取隔离时的拒绝行为，Windows 尚未纳入 CI。跨平台桌面运行与工具执行能力仍需分别验证。

## 核心能力

| 能力 | 你可以做什么 |
| --- | --- |
| 按需组队 | 从产品策划、UX、UI、前端、后端、QA、技术负责人、架构师、运维、项目经理等角色中选择成员，并为角色绑定不同模型。 |
| 分阶段协作 | 用流程包组织开发，按阶段激活 Agent；小任务可走快速通道。项目经理负责派活，产物负责交接。 |
| 可核对的交付 | 规格、设计和代码落盘；复审与检验记录绑定具体产物版本，修改后需要重新核对证据。 |
| 人在回路 | 在待决卡中处理授权与裁决，查看归来摘要，暂停团队；最终验收由负责人完成。 |
| 本机工作区 | 在同一工作台查看群聊、文件树、代码、产物、执行轨迹与模型用量。流程状态保存在本机 SQLite。 |
| 浏览器与电脑操作 | 连接浏览器、查看电脑就绪状态，并在项目权限范围内授权操作。原生电脑操作目前仅支持 macOS。 |
| 可扩展工具 | 通过技能与 MCP 服务补充角色能力，并按项目管理工具权限。 |
| 多语言界面 | 支持七种界面语言；项目内容和产物保持其原本语言。 |

### 一次项目如何推进

1. **选项目、组团队**：创建项目或选择现有仓库，配置模型与角色。
2. **说明目标**：写下需要完成的工作与验收要求，在群聊中补充约束。
3. **查看交付**：Agent 按流程产出文件、复审并执行检查，工作台记录过程。
4. **处理待决事项**：批准必要操作、回答问题或调整方向。
5. **最终验收**：核对可运行代码与检验证据，再决定是否接受交付。

本地合入、`git push` 和远程发布是不同操作；远程动作需要明确指令与相应授权。

## 快速开始

### 准备环境

- **Node.js 24 与 npm**：与当前 CI 使用的版本一致。
- **Rust stable**：通过 rustup 安装，并包含 `rustfmt`、`clippy`。
- **Git**。
- **Tauri 2 系统依赖**：按操作系统完成[官方前置环境配置](https://v2.tauri.app/start/prerequisites/)。macOS 桌面目标可使用 Xcode Command Line Tools。
- **macOS 上另需 macOS 15+ 与 Swift 6.2+**：开发与构建命令还会编译原生电脑操作模块。这些额外要求来自其 [Swift 包清单](native/computer-use/Package.swift)；当前选定的开发工具链须提供兼容的 `swift` 可执行文件。
- 一个可用的模型服务，以及它所需的凭据；模型调用费用由所选服务决定。

### 启动工作台

```bash
git clone https://github.com/zxzAndyMAC/Hexagon-Bot.git
cd Hexagon-Bot
npm ci --prefix ui
npm run dev
```

`npm run dev` 会在 macOS 编译原生电脑操作模块，准备固定版本浏览器运行时，再启动 Vite（端口 `1420`）并打开 Tauri 桌面窗口。全新检出需要联网获取浏览器依赖、Chromium 和 Rust/Swift 依赖，并等待首次编译完成。

如果 macOS / Linux 终端找不到 `cargo`，先执行：

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

打开工作台后，在设置中配置模型服务，再通过项目向导选择目录、团队和流程。可以从一个验收要求明确的小任务开始，例如“为现有页面增加搜索，并补上测试”。

关闭已编辑文件或切换项目前，未保存内容会提供保存、放弃、取消三种选择；进入设置页会保留脏稿。如果文件已在盘上改变，保存会保留盘上版本，等待你处理冲突。

### 继续中断的工作

如果卡片显示**裁决已生效，原任务待继续**，此前裁决已经执行。配置可用模型后，通过卡片的重试操作继续原任务。对于执行结果未知的动作，重试前先核对执行记录，因为它可能已经产生效果。

在 macOS 使用原生电脑操作前，请查看工作台的电脑状态，并授予所需的辅助功能与屏幕录制权限。系统权限与项目动作审批分别管理；就绪状态本身不代表授权。

### 代码库探索

中文问题会由已配置的 Agent 模型推导候选标识符，再通过限定范围的 `fs_find` / `fs_grep` 和 `fs_read` 阅读实际实现，无需另外配置嵌入模型或安装模型权重。

`fs_grep` 每次搜索一个字面子串，不支持正则表达式或用 `|` 表示多个候选。结果最多 100 条；判断未发现前，先检查 `coverage` 中的截断和跳过文件。

`sem_search` 保留为可选的字符相似度兼容工具，不能把中文概念翻译成英文代码，也不能证明语义正确。它报告 `hash-ngram-v1`，保留字面匹配并复用未改变的索引文件；刷新时通过引擎签名替换旧神经向量。此前安装的模型文件保留，但不再加载。详见[历史检索测量与限制](evaluation/retrieval/README.md)。

需要附源码依据的回答时，以 `/source ` 开头输入问题，也可以点名团队成员。该模式要求引用，例如 `src/example.rs:12`，与本回合实际读取的源码行对应；其他可识别的路径引用也会检查，请在反引号内使用完整的仓库相对路径。当前语法不能完整解析正文里的行号、没有扩展名的根目录文件名，或反引号外含空格的路径。

在既有回合上限内，交付前允许一次证据修补，以及一次单独的主张与源码核对修订。包含肯定结论的草稿会在新的模型上下文中复核：保留负责人和系统指令，要求重新读取源码，并省略旧草稿，避免受其结论影响。唯一文件名可根据保留的可用读取路径补全，存在歧义则拒绝。

如果没有找到实现，模型可以报告未发现，由工作台根据实际搜索范围与读取区间呈现，不能据此声称全仓不存在。证据缺失或修订未完成时，调查保持未验证。这些检查与修订不保证事实正确性；普通聊天不启用此门禁。文本读取结果的行号是展示元数据，不属于文件内容，复制源码进行修改时应去掉行号前缀。

### 数据与凭据

项目文件、流程状态和轨迹保存在本机；模型请求与外部工具调用会把所需内容发送给你配置的服务。本机存储不等于所有模型推理都离线进行。

模型凭据的存储方式以设置界面显示为准：支持系统凭据库，也有开发用明文文件模式。MCP 环境变量与认证标头目前保存在本机配置文件中。请勿将这些配置、密钥或个人轨迹提交到公共仓库。

## 开发与贡献

欢迎通过 [Issues](https://github.com/zxzAndyMAC/Hexagon-Bot/issues) 反馈问题或提出建议，也欢迎提交 Pull Request。问题报告请附复现步骤、操作系统、预期与实际结果，并清理日志中的敏感信息。

### 常用命令

以下命令均在仓库根目录执行：

| 命令 | 用途 |
| --- | --- |
| `npm run dev` | 启动前端开发服务与桌面窗口。 |
| `npm run check` | 检查 Rust 格式与 Clippy、架构规则、前端 lint 与类型、设计 token、多语言 key 和生成类型漂移。 |
| `npm test` | 运行 Rust workspace、前端 Vitest、浏览器运行时及平台电脑操作测试。 |
| `npm run test:browser` | 准备固定版本浏览器运行时，运行本地边界与生命周期测试。 |
| `npm run test:computer-use` | 测试 SDK 准备逻辑；在 macOS 校验固定 SDK 补丁并运行 Swift 原生测试。 |
| `npm run build` | 构建前端与 Rust release 产物；当前配置不生成桌面安装包。 |
| `cargo test export_bindings` | 修改跨 IPC 的 Rust DTO 后，重新生成前端类型绑定。 |

代码变更提交前请运行 `npm run check` 和 `npm test`；修复问题时附回归测试。

### 仓库结构

```text
crates/hexagon-core/  业务内核：编排、回合、产物、权限、用量与轨迹
src-tauri/           桌面壳：窗口与 IPC 转发
ui/                  React + TypeScript 界面
ui/src/gen/          从 Rust DTO 生成的 TypeScript 类型
native/browser/      浏览器运行时及边界、生命周期测试
native/computer-use/ Swift 电脑操作模块、固定 SDK 补丁及原生测试
evaluation/         任务对照与检索评测材料
scripts/            架构、绑定及其他开发检查
```

业务逻辑集中在 `hexagon-core`，桌面壳调用其门面，UI 通过 IPC 使用生成的类型。开发约定见 [AGENTS.md](AGENTS.md)，领域术语见 [CONTEXT.md](CONTEXT.md)，评测方式见 [evaluation/README.md](evaluation/README.md)。

## 许可证

Hexagon-Bot 使用 **[MIT License](LICENSE)**，版权归 Hexagon-Bot contributors 所有。

第三方依赖与素材保留各自的许可证。随仓库分发的 Material Icon Theme 图标，其版权与许可全文见 [图标许可证](ui/src/assets/fileicons/LICENSE)。

## 致谢与参考

感谢以下研究者、项目作者与维护者。Hexagon-Bot 的实现、设计取舍和研究过程受益于这些工作；下面分别列出设计参考与实际使用的开源组件。

### 论文与研究

| 工作 | 对本项目的启发 |
| --- | --- |
| [MetaGPT: Meta Programming for A Multi-Agent Collaborative Framework](https://arxiv.org/abs/2308.00352) · [源码](https://github.com/FoundationAgents/MetaGPT) | 以标准流程组织角色，用结构化文档与产物交接软件开发工作。 |
| [ChatDev: Communicative Agents for Software Development](https://arxiv.org/abs/2307.07924) · [源码](https://github.com/OpenBMB/ChatDev) | 分阶段的软件开发协作，以及角色沟通边界的设计。 |
| [Dream-RSI: Recursive Self-Improvement through Evolving Worlds](https://github.com/zhengkid/Dream-RSI) | 编排策略、历史回放与候选评估的研究参考；帮助明确“回放证据不等于真实任务收益”的边界。 |
| [RSIAgent](https://github.com/AetherLabsAI/RSIAgent) | 可复用经验、执行与验证分离、冻结评测的研究参考。 |

这些引用说明设计与研究来源，不代表复现了论文结果，也不构成 Hexagon-Bot 的性能或自改进效果声明。

### 工程借鉴

- **[OpenWorker](https://github.com/andrewyng/openworker)**：命令权限检查、独立审查者、授权来源追踪、技能管理与过长工具结果处理。相关 Rust 模块保留了具体出处注释。
- **[aisuite](https://github.com/andrewyng/aisuite)**：多供应商模型调用与流式工具调用的研究参考。
- **[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)**：执行轨迹、请求可核对性、回放与受控扩展机制的研究参考。

### 开源组件与素材

| 用途 | 项目 |
| --- | --- |
| 桌面与界面 | [Tauri](https://github.com/tauri-apps/tauri)、[React](https://github.com/facebook/react)、[Vite](https://github.com/vitejs/vite)、[Tailwind CSS](https://github.com/tailwindlabs/tailwindcss)、[Zustand](https://github.com/pmndrs/zustand)、[Motion](https://github.com/motiondivision/motion) |
| 编辑、阅读与图表 | [Monaco Editor](https://github.com/microsoft/monaco-editor)、[Shiki](https://github.com/shikijs/shiki)、[react-markdown](https://github.com/remarkjs/react-markdown)、[remark-gfm](https://github.com/remarkjs/remark-gfm)、[React Virtuoso](https://github.com/petyosi/react-virtuoso)、[Apache ECharts](https://github.com/apache/echarts) |
| 国际化 | [i18next](https://github.com/i18next/i18next)、[react-i18next](https://github.com/i18next/react-i18next) |
| 持久化、协议与类型 | [SQLite](https://sqlite.org/)、[rusqlite](https://github.com/rusqlite/rusqlite)、[Serde](https://github.com/serde-rs/serde)、[ts-rs](https://github.com/Aleph-Alpha/ts-rs)、[ureq](https://github.com/algesten/ureq)、[jsonschema](https://github.com/Stranger6667/jsonschema) |
| 凭据、检索与上下文 | [keyring-rs](https://github.com/open-source-cooperative/keyring-rs)、[ignore / ripgrep](https://github.com/BurntSushi/ripgrep)、[tiktoken-rs](https://github.com/zurawiki/tiktoken-rs) |
| 测试与质量检查 | [Vitest](https://github.com/vitest-dev/vitest)、[Proptest](https://github.com/proptest-rs/proptest)、[Oxc / Oxlint](https://github.com/oxc-project/oxc) |
| 文件图标 | [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) |

协议与扩展设计也参考了 [Model Context Protocol](https://modelcontextprotocol.io/docs/getting-started/intro) 与 [Agent Skills](https://agentskills.io/home)。完整直接依赖以 [Rust 清单](crates/hexagon-core/Cargo.toml)、[桌面壳清单](src-tauri/Cargo.toml)和[前端清单](ui/package.json)为准。

README 的内容组织参考了 [Ollama](https://github.com/ollama/ollama)、[Tauri](https://github.com/tauri-apps/tauri) 与 [Dify](https://github.com/langgenius/dify)：先说明用途，再提供启动路径、开发入口与项目来源。感谢所有为这些基础工作投入时间的贡献者。
