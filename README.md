<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="Hexagon-Bot logo" />
</p>

<h1 align="center">Hexagon-Bot</h1>

<p align="center">
  <strong lang="en">English</strong> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a>
</p>

<p align="center">
  <strong>Build an AI software development team on your machine. Turn requirements into working code.</strong>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/zxzAndyMAC/Hexagon-Bot/actions/workflows/ci.yml"><img src="https://github.com/zxzAndyMAC/Hexagon-Bot/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/status-early%20development-e8a33d" alt="Status: early development" /></a>
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#development-and-contributing">Development and contributing</a> ·
  <a href="#license">License</a> ·
  <a href="#acknowledgments-and-references">Acknowledgments and references</a>
</p>

Hexagon-Bot is a local desktop workbench for software development. As the project owner, you choose roles, assign models, and select a project directory. Agents work through a defined workflow, handing off specifications, designs, code, and test records. You can redirect the work, resolve pending decisions, and perform final acceptance at any time.

**The project is in early development. The instructions below run it from source.** Full desktop quality checks currently run in macOS CI. Linux has dedicated isolation tests; Windows is not yet covered by CI. Desktop operation and tool execution still need separate verification on each platform.

## Features

| Capability | What you can do |
| --- | --- |
| Assemble your team | Choose product planning, UX, UI, frontend, backend, QA, technical lead, architect, operations, and project manager roles, and assign different models to them. |
| Work in stages | Organize development with workflow packs that activate agents by stage, or use the fast track for smaller tasks. The project manager assigns work; artifacts carry the handoffs. |
| Inspect deliverables | Specifications, designs, and code are saved to disk. Review and validation records are tied to specific artifact versions, so changes require fresh evidence. |
| Stay in control | Handle permissions and decisions through pending cards, read a return summary, or pause the team. Final acceptance stays with you. |
| Use a local workspace | View the group chat, file tree, code, artifacts, execution traces, and model usage in one workbench. Workflow state is stored locally in SQLite. |
| Extend the tools | Add capabilities through skills and MCP services, with tool permissions managed per project. |
| Choose your language | Use the interface in seven languages while keeping project content and artifacts in their original language. |

### How a project moves forward

1. **Choose a project and team.** Create a project or select an existing repository, then configure models and roles.
2. **Describe the goal.** Define the work and acceptance criteria, and add constraints in the group chat.
3. **Inspect the deliverables.** Agents produce files, review work, and run checks while the workbench records their progress.
4. **Resolve pending decisions.** Authorize necessary actions, answer questions, or change direction.
5. **Accept the result.** Review the working code and validation evidence before accepting the delivery.

Local merges, `git push`, and remote publishing are separate operations. Remote actions require explicit instructions and the appropriate authorization.

## Quick start

### Prerequisites

- **Node.js 24 and npm**, matching the current CI environment.
- **Stable Rust**, installed through rustup, including `rustfmt` and `clippy`.
- **Git**.
- **Tauri 2 system dependencies**: follow the [official prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system. Xcode Command Line Tools are sufficient for desktop development on macOS.
- A working model service and any credentials it requires. Model usage costs depend on your chosen provider.

### Start the workbench

```bash
git clone https://github.com/zxzAndyMAC/Hexagon-Bot.git
cd Hexagon-Bot
npm ci --prefix ui
npm run dev
```

`npm run dev` starts Vite on port `1420` and opens the Tauri desktop window. The first launch needs to compile the Rust dependencies.

If your macOS or Linux terminal cannot find `cargo`, run:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Once the workbench opens, configure your model service in settings. Then use the project wizard to select a directory, team, and workflow. Start with a small task that has clear acceptance criteria, such as “add search to the existing page and cover it with tests.”

Before closing an edited file or switching projects, choose Save, Discard, or Cancel for unsaved changes. Visiting settings keeps your draft. If the file changed on disk, saving preserves that version and asks you to resolve the conflict.

### Repository exploration

Chinese questions use the configured agent model to derive candidate identifiers,
then scoped `fs_find` / `fs_grep` and `fs_read` to inspect actual implementations.
No separate embedding model or model-weight installation is required.
`sem_search` remains an optional character-similarity tool for compatibility; it
cannot translate Chinese concepts into English code or establish semantic truth.
It reports `hash-ngram-v1`, preserves literal matches and reuses unchanged indexed
files. Old neural vectors are replaced on refresh through engine signatures.
Previously installed model files are left untouched and are no longer loaded.
See the [historical retrieval measurements and limits](evaluation/retrieval/README.md).

For a source-backed answer, start a message with `/source ` followed by your
question (you can also mention a team member). This mode requires a citation
such as `src/example.rs:12` matching source lines actually read in that turn.
Additional recognizable path references are checked too; use full repository-relative
paths in backticks. This grammar does not comprehensively parse prose line numbers,
extensionless root filenames, or space-containing paths outside backticks.
It allows one evidence repair attempt and a separate claim-to-source revision
before delivery, within the existing turn limit. Positive drafts are reviewed in a
fresh model context that preserves owner/system instructions but requires new source
reads; the prior draft is omitted to avoid anchoring on its claims. Unique basenames may be expanded
from retained usable read paths; ambiguous names are refused. If no implementation is located,
the model can return a non-finding that the workbench renders from actual search
scopes and read ranges, without claiming repository-wide absence. Missing evidence
or an unfinished revision leaves the investigation unverified. These checks and
the revision do not guarantee factual correctness. Ordinary chat does not enable
this gate. Text read results show line numbers as display metadata; these prefixes
are not part of the file and must be omitted when copying source into edits.

### Data and credentials

Project files, workflow state, and traces are stored locally. Model requests and external tool calls send the necessary content to the services you configure. Local storage does not mean all model inference runs offline.

Check settings for the active model credential store: the application supports the system credential store and a plaintext file mode for development. MCP environment variables and authentication headers are currently stored in local configuration files. Keep these configurations, secrets, and personal traces out of public repositories.

## Development and contributing

Bug reports and suggestions are welcome through [Issues](https://github.com/zxzAndyMAC/Hexagon-Bot/issues), as are pull requests. Include reproduction steps, your operating system, and expected versus actual behavior. Remove sensitive information from logs before sharing them.

### Common commands

Run these commands from the repository root:

| Command | Purpose |
| --- | --- |
| `npm run dev` | Start the frontend development server and desktop window. |
| `npm run check` | Check Rust formatting and Clippy, architecture rules, frontend lint and types, design tokens, translation keys, and generated type drift. |
| `npm test` | Run Rust workspace, frontend Vitest, browser runtime, and platform computer-use tests. |
| `npm run test:browser` | Prepare the pinned browser runtime and run its local boundary and lifecycle tests. |
| `npm run build` | Build the frontend and Rust release artifacts. The current configuration does not produce desktop installers. |
| `cargo test export_bindings` | Regenerate frontend type bindings after changing Rust DTOs used across IPC. |

Run `npm run check` and `npm test` before submitting code changes, and include regression tests with bug fixes.

### Repository layout

```text
crates/hexagon-core/  Business logic: orchestration, turns, artifacts, permissions, usage, and traces
src-tauri/           Desktop shell: windows and IPC forwarding
ui/                  React + TypeScript interface
ui/src/gen/          TypeScript types generated from Rust DTOs
evaluation/          Task comparison and retrieval evaluation materials
scripts/             Architecture, binding, and other development checks
```

Business logic lives in `hexagon-core`. The desktop shell calls its facade, and the UI communicates over IPC using generated types. See [AGENTS.md](AGENTS.md) for development conventions, [CONTEXT.md](CONTEXT.md) for domain terminology, and [evaluation/README.md](evaluation/README.md) for evaluation procedures. These supporting documents are currently written in Chinese.

## License

Hexagon-Bot is licensed under the **[MIT License](LICENSE)**. Copyright belongs to the Hexagon-Bot contributors.

Third-party dependencies and assets retain their own licenses. The Material Icon Theme assets distributed in this repository include their full copyright notice and license in the [icon license file](ui/src/assets/fileicons/LICENSE).

## Acknowledgments and references

Thank you to the researchers, authors, and maintainers whose work informed Hexagon-Bot's implementation, design choices, and research. Design references and the open-source components used by the project are listed separately below.

### Papers and research

| Work | Influence on this project |
| --- | --- |
| [MetaGPT: Meta Programming for A Multi-Agent Collaborative Framework](https://arxiv.org/abs/2308.00352) · [Source](https://github.com/FoundationAgents/MetaGPT) | Organizing roles through standard workflows and handing off software development work through structured documents and artifacts. |
| [ChatDev: Communicative Agents for Software Development](https://arxiv.org/abs/2307.07924) · [Source](https://github.com/OpenBMB/ChatDev) | Staged software development collaboration and boundaries for communication between roles. |
| [Dream-RSI: Recursive Self-Improvement through Evolving Worlds](https://github.com/zhengkid/Dream-RSI) | Research into orchestration policies, historical replay, and candidate evaluation, including the distinction between replay evidence and gains on real tasks. |
| [RSIAgent](https://github.com/AetherLabsAI/RSIAgent) | Research into reusable experience, separating execution from verification, and keeping evaluation frozen. |

These references credit design and research sources. They do not imply that Hexagon-Bot reproduces the papers' results or establish its performance or self-improvement effectiveness.

### Engineering influences

- **[OpenWorker](https://github.com/andrewyng/openworker)**: command permission checks, independent review, authorization provenance, skill management, and handling oversized tool results. The relevant Rust modules retain comments identifying their sources.
- **[aisuite](https://github.com/andrewyng/aisuite)**: research into model calls across providers and streaming tool calls.
- **[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)**: research into execution traces, verifiable requests, replay, and governed extension mechanisms.

### Open-source components and assets

| Purpose | Projects |
| --- | --- |
| Desktop and interface | [Tauri](https://github.com/tauri-apps/tauri), [React](https://github.com/facebook/react), [Vite](https://github.com/vitejs/vite), [Tailwind CSS](https://github.com/tailwindlabs/tailwindcss), [Zustand](https://github.com/pmndrs/zustand), [Motion](https://github.com/motiondivision/motion) |
| Editing, reading, and charts | [Monaco Editor](https://github.com/microsoft/monaco-editor), [Shiki](https://github.com/shikijs/shiki), [react-markdown](https://github.com/remarkjs/react-markdown), [remark-gfm](https://github.com/remarkjs/remark-gfm), [React Virtuoso](https://github.com/petyosi/react-virtuoso), [Apache ECharts](https://github.com/apache/echarts) |
| Internationalization | [i18next](https://github.com/i18next/i18next), [react-i18next](https://github.com/i18next/react-i18next) |
| Persistence, protocols, and types | [SQLite](https://sqlite.org/), [rusqlite](https://github.com/rusqlite/rusqlite), [Serde](https://github.com/serde-rs/serde), [ts-rs](https://github.com/Aleph-Alpha/ts-rs), [ureq](https://github.com/algesten/ureq), [jsonschema](https://github.com/Stranger6667/jsonschema) |
| Credentials, search, and context | [keyring-rs](https://github.com/open-source-cooperative/keyring-rs), [ignore / ripgrep](https://github.com/BurntSushi/ripgrep), [tiktoken-rs](https://github.com/zurawiki/tiktoken-rs) |
| Testing and quality checks | [Vitest](https://github.com/vitest-dev/vitest), [Proptest](https://github.com/proptest-rs/proptest), [Oxc / Oxlint](https://github.com/oxc-project/oxc) |
| File icons | [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) |

Protocol and extension design also draws on [Model Context Protocol](https://modelcontextprotocol.io/docs/getting-started/intro) and [Agent Skills](https://agentskills.io/home). For the complete direct dependency lists, see the [Rust manifest](crates/hexagon-core/Cargo.toml), [desktop shell manifest](src-tauri/Cargo.toml), and [frontend manifest](ui/package.json).

This README's organization takes inspiration from [Ollama](https://github.com/ollama/ollama), [Tauri](https://github.com/tauri-apps/tauri), and [Dify](https://github.com/langgenius/dify): explain the purpose first, then provide a path to getting started, contributing, and exploring the project's foundations. Thank you to everyone who contributes to this work.
