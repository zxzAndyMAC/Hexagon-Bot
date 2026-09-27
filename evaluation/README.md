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
