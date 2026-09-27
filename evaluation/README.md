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

debug 夹具仅用于工具冒烟，不计入规格要求的二十个任务。完整对照、预算、停止与恢复、策略质量门及真实实验按后续票据实施。

类别清单可通过同一宿主入口执行自检：

```sh
cargo run -p hexagon-core --example evaluate -- check-category /tmp/hexagon-evaluation evaluation/corpus/bugs.json
```

清单固定为五个不同任务、三开发/两留出及一项开发试跑。输出 `fixture_self_check` 报告逐项保留起点、参考修复、关键错误修复的独立验收事实；任意自检不符返回非零状态。此结果只证明任务可验收，不是模型收益。任务来源与 MIT 许可随每个输入副本提供，依赖只用声明的标准库，不下载包。正式批次需另行冻结实际解释器版本与运行环境。


类别任务还须提供非空 `expected_stdout`：宿主核对完整实际输出，该预期值不注入验收进程。Python 探针仅输出函数行为数据，不在执行任务代码的进程内断言成功；提前退出、输出缺项及额外输出均失败。macOS 使用 Xcode 自带的实际 Python 解释器，绕开依赖 posix_spawn 的启动包装器，进程隔离规则保持生效。

`corpus/features.json` 提供五个带测试的功能任务；独立功能探针之外，提交的 unittest 至少三项通过且必须发现预设回归。缺测试或只改测试断言不能替代功能交付。探针规范化等值整数/浮点及对象键顺序，避免序列化形式制造失败。

`corpus/interfaces.json` 提供五个 Python 服务端与 CommonJS 客户端任务，实际服务端数据经管道送入客户端。宿主同时核对完整报文与客户端结果，单改一侧、保留旧字段或不兼容响应均不能通过。两个清单也使用 `check-category` 入口，仍属于夹具自检。

`corpus/dirty-trees.json` 的五项任务声明 `initial_changes`（已跟踪文件的原有改动）、`untracked_files` 和可选 `preserved_fragments`（允许修改的文件内仍须保留的内容）。宿主仅在新副本初始化固定 Git 基线，再应用原有改动。Git 初始化不读取个人配置，不执行个人模板、签名或 hooks。起始 HEAD、索引和配置指纹在任务派遣前保存；验收同时核对原有文件内容和 Git 未提交状态。目标功能通过也不能抵消删除、回退、误暂存或误提交已有工作。

批次配置通过 `freeze HOST CONFIG.json [PARENT_BATCH]` 冻结，`batch HOST BATCH_ID` 重读，`check-config HOST BATCH_ID CONFIG.json` 核对当前配置与环境。核对拒绝时输出结构化 `blocks` 并返回非零状态。变更任务、验收、模型绑定、角色、流程、限制或统计规则须另建关联批次；旧批次保持原始指纹。此入口不执行付费预检。

`CONFIG.json` 对应 `FreezeRequest`：四份完整 `corpora`、现有 `main_slot`、`fast_role`、完整 `full_pack`、`prices`、`limits` 及 `statistics_version: "paired-benefit-v1"`。金额单位为千分之一美分（`total_mc: 20000000` = 200 美元，`pilot_mc: 2000000` = 20 美元，`run_mc: 500000` = 5 美元）；`requests: 80`、`active_ms: 1800000`。正式配置必须使用开发阶段选定的现有角色与流程包，不能以测试用简化流程代替。

冻结记录包含实际槽绑定、供应商和型号、输出界限、角色能力、宿主判定与隔离约定、二进制指纹、解释器及系统环境。供应商默认生成参数标为未指定，不虚构具体温度。URL 带凭据、查询串或片段时不作为可用身份；钥匙正文不进入配置。

`record-verification HOST BATCH_ID OBSERVATION.json` 保存来源核对观察（`dimension: model|tools|price`、`outcome: unknown|failed|reported_pass`、`batch_fingerprint`、不带秘密的 `source_url`、`checked_at`）。观察与可计费调用凭据分开：报告通过不会清除 `model_not_verified`、`tools_not_verified` 或 `price_not_verified`。正式准入还需后续隔离、共享预算与停止机制，当前以 `execution_guards_pending` 明示，配置存在不等于可调用。

`plan HOST BATCH_ID pilot|formal` 保存配对顺序，重复调用返回原计划。正式计划为 8 个留出任务 × 3 次 × 两侧共 48 次，整体各 12 对由快速通道/完整流程先行。`read-plan HOST PLAN_ID` 保留每次的任务、重复号、路径、状态、运行身份及未启动原因；重读不洗牌。

冻结配置可通过 `task_owners` 给选定的既有角色分配各任务 `allowed_paths` 中的精确文件，适用于这些文件不在预置目录归属内的夹具。此项在运行前选定并冻结，两侧共用；原角色职责、技能、既有归属及内置拒绝仍然有效。缺省不额外分配，不能通过清空归属来放行任务。

`next-debug HOST PLAN_ID SCRIPT.json` 仅启动持久顺序中的下一侧。脚本为 `[{"role":"后端","writes":{"目标文件":"内容"}}]`，完整流程按各阶段角色顺序提供激活脚本；不改文件的激活使用空 `writes`。快速通道使用选定的实例，完整流程实际派遣角色、推进阶段并通过宿主盖章门面进行脚本裁决。缺交付、检验或复审时保留未完成。脚本裁决不计真人证据，所有这类结果仍标 `scripted_debug`。

每次运行重建独立工作副本、数据库、会话和经验目录。受保护的 `.hexagon/evaluation-worker` 标记阻止继承全局可变技能与开关；内置技能仍由固定程序版本提供。计划只保存身份及状态，不复用上一侧的交付内容。

`stop-plan HOST PLAN_ID` 原子停止后续调度：所有尚未启动项保留为 `not_run / owner_stopped`，已启动与终态事实保留，重开后不能悄悄续跑该计划。此命令当前只停止新增调度；在途取消与费用收场由后续停止机制接通。

`generation HOST BATCH_ID` 创建与执行侧分离的新生成上下文，只有 12 个开发任务的公共字段及基线流程包；隐藏验收、预期输出、正确/错误参考均不复制。`read-generation HOST CONTEXT_ID` 检查材料指纹与污染状态。此入口只准备材料；付费生成和候选资格由后续候选链路处理。

向生成方提供留出反馈时，用 `reveal-task HOST CONTEXT_ID TASK_ID` 记录揭示：该上下文不能再作为干净生成来源，相关任务内容退役为回归用途。揭示与实际运行使用分别持久记录。任务的身份、分发元数据、环境/归属声明不能建立“新”留出内容；改名、改版本描述或重排依赖后，新批次仍会得到 `heldout_retired`，正式计划不能启动。配置指纹仍完整冻结这些声明。

`check-isolation HOST` 运行固定边界探针并保存记录：公共需求必须可读，工具及终端/子进程不能读写外部隐藏文件，评测标记不能被改写，索引和模型请求不携带隐藏正文。正向对照防止把“进程根本没启动”当成隔离通过。`read-isolation HOST CHECK_ID` 重读绑定当前可执行文件指纹的宿主探针证据；该证据只证明被测边界，不是任务通过、真人收益或模型能力证明。

真人处理入口使用 `next-owner-debug HOST PLAN_ID SCRIPT.json` 开始一条保留真实待决的脚本运行，再用 `owner-session HOST RUN_ID` 处理。会话内 `start`/`continue` 开始计时，`guidance TEXT` 把额外指导放入该运行的普通负责人消息，`away` 结束区间，`timing` 显示分项时间，`quit` 离席。完整流程的 `approve` 调用原有盖章门面；`decision JSON` 提交带类型的裁决。快速通道的交付检查使用 `{"kind":"finish_review"}`，不会盖流程章或合入代码。非交互 stdin 标为 scripted；脚本模型运行即使由真人点击，也不能成为真实模型收益样本。

只有开始处理后才能手改打印出的独立运行目录。处理前后记录文件指纹、额外指导次数与单调时钟时长；离席期间发生文件变化会永久标记漏计。单个宿主同一时刻只能有一个负责人处理区间，结束的句柄不可再提交。待人状态跨重开保留原运行、阶段和目录。`timing HOST RUN_ID` 分开显示人工时间、活动时间、等待和总历时；脚本或不完整计时的 `human_ms` 为 null，不能用零代替。

`pending HOST RUN_ID` 读取该运行真实待决卡；会话也显示卡号。权限裁决用 `{"kind":"permission","question_id":"…","allow":false}`，最终驳回用 `{"kind":"reject_final","stage":"…","note":"…"}`。返工期间可在处理区间内修改当前副本并补充指导，再用 `{"kind":"continue_rework"}` 逐阶段重新检查；这不会重放原脚本。只有完整流程真实完成才设置 `flow_completed`，快速通道的交付复核不会设置它。
