# 任务收益评测

当前入口仅支持离线脚本调试和持久结果读取，不调用真实模型、不证明真人收益。

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo run -p hexagon-core --example evaluate -- debug /tmp/hexagon-evaluation evaluation/debug/task.json evaluation/debug/writes.json
cargo run -p hexagon-core --example evaluate -- read /tmp/hexagon-evaluation eval-1
```

请使用独立宿主目录；`eval-1` 替换为实际返回的运行身份。每次创建新工作副本，先保存启动记录，再由工作台的实例派遣和普通读写工具执行脚本。结果保留在宿主数据库，重开可读取。

任务声明包含可分发来源、许可、固定版本、起始文件、需求、允许修改路径、正确参考与关键错误参考。`validator` 是 `validation_files` 中的 shell 入口路径。宿主验收材料不得与起始文件或允许修改路径重叠；只注入另建的验收副本，不交给执行 Agent。启动前核对起点失败、参考通过、错误参考失败。

结果分别呈现执行状态、流程完成、独立验收与负责人例外。验收保存命令、退出状态、输出、文件和权限指纹、起止时间；测中修改文件或权限不能通过。离线脚本没有完成负责人最终验收，流程完成与例外不会凭脚本结束自动设为真。

debug 夹具仅用于工具冒烟，不计入规格要求的二十个任务。完整对照、预算、停止与恢复、真人计时、策略质量门及真实实验按后续票据实施。

类别清单可通过同一宿主入口执行自检：

```sh
cargo run -p hexagon-core --example evaluate -- check-category /tmp/hexagon-evaluation evaluation/corpus/bugs.json
```

清单固定为五个不同任务、三开发/两留出及一项开发试跑。输出 `fixture_self_check` 报告逐项保留起点、参考修复、关键错误修复的独立验收事实；任意自检不符返回非零状态。此结果只证明任务可验收，不是模型收益。任务来源与 MIT 许可随每个输入副本提供，依赖只用声明的标准库，不下载包。正式批次需另行冻结实际解释器版本与运行环境。


类别任务还须提供非空 `expected_stdout`：宿主核对完整实际输出，该预期值不注入验收进程。Python 探针仅输出函数行为数据，不在执行任务代码的进程内断言成功；提前退出、输出缺项及额外输出均失败。macOS 使用 Xcode 自带的实际 Python 解释器，绕开依赖 posix_spawn 的启动包装器，进程隔离规则保持生效。

`corpus/features.json` 提供五个带测试的功能任务；独立功能探针之外，提交的 unittest 至少三项通过且必须发现预设回归。缺测试或只改测试断言不能替代功能交付。探针规范化等值整数/浮点及对象键顺序，避免序列化形式制造失败。

`corpus/interfaces.json` 提供五个 Python 服务端与 CommonJS 客户端任务，实际服务端数据经管道送入客户端。宿主同时核对完整报文与客户端结果，单改一侧、保留旧字段或不兼容响应均不能通过。两个清单也使用 `check-category` 入口，仍属于夹具自检。

`corpus/dirty-trees.json` 的五项任务声明 `initial_changes`（已跟踪文件的原有改动）、`untracked_files` 和可选 `preserved_fragments`（允许修改的文件内仍须保留的内容）。宿主仅在新副本初始化固定 Git 基线，再应用原有改动。Git 初始化不读取个人配置，不执行个人模板、签名或 hooks。起始 HEAD、索引和配置指纹在任务派遣前保存；验收同时核对原有文件内容和 Git 未提交状态。目标功能通过也不能抵消删除、回退、误暂存或误提交已有工作。
