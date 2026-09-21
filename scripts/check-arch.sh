#!/usr/bin/env bash
# 架构接缝检查面固化（QA 委托 GR-06 / arch-review 治理沉淀执行化）：
# AGENTS.md「架构接缝规则」各条的 rg 检查面从「评审时记得跑」变成
# 「npm run check 拦住」。命中判定三原则：白名单文件、豁免注释标签
# （D01-ok / D01-exempt / D13-exempt，命中行当行或紧邻下一行）、注释行不算命中。
# 豁免必须行内留理由——注释跟着代码走，不在评审嘴里。
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

deny() { # $1=规则号 $2=说明 $3=违规命中（空=全合规）
  if [ -n "$3" ]; then
    echo "✗ $1: $2"; echo "$3" | sed 's/^/    /'; fail=1
  else
    echo "✓ $1: $2"
  fi
}

# D01 read 组禁触 state.wb：每处命中的当行或紧邻下一行必须挂
# D01-ok（mutation 路径理由）或 D01-exempt（read 侧探针，不调 wb
# 方法）。generate_handler! 是扁平单表，三组只是约定——标签是
# 唯一可机检边界；窗口取当行+下行是迁就 rustfmt 对 match { 后
# 注释的排版（它会把行尾注释挪进块首行）。
d01bad=""
while IFS=: read -r ln rest; do
  [ -z "$ln" ] && continue
  sed -n "${ln}p;$((ln + 1))p" src-tauri/src/lib.rs | grep -q 'D01-\(ok\|exempt\)' \
    || d01bad="$d01bad\n  src-tauri/src/lib.rs:$ln:$rest"
done < <(rg -n 'state\.wb' src-tauri/src/lib.rs)
deny D01 "state.wb hits must carry D01-ok / D01-exempt (same or next line)" \
  "$(printf '%b' "$d01bad")"

# D04/D09 pending_questions 与 agents.status 写路径只落属主模块
# （cards.rs / orchestra.rs）；合法豁免=测试 helper、schema 断言、
# credentials.rs 泄漏扫描标签、注释行。
deny D04_D09 "pending_questions/agents.status writes stay in owner modules" \
  "$(rg -n 'pending_questions|UPDATE agents SET status' crates/hexagon-core/src \
     | rg -v 'src/(cards|orchestra|db|credentials)\.rs:|src/api/tests\.rs:' \
     | rg -v ':[0-9]+:\s*//')"

# D06 IPC 返回值禁裸 Value：api.rs 内 Result<Vec<Value>/Result<Value 归零。
deny D06 "no raw Value IPC returns in api.rs" \
  "$(rg -n 'Result<Vec<Value>|Result<Value' crates/hexagon-core/src/api.rs)"

# D08 门面依赖白名单：use crate::api 只许 scenario/setup/replay；
# crate::api:: 路径引用除 Workbench（for_test 测试接缝，cfg(test) 编译闸
# 天然兜底）外只许落白名单 + errcode.rs（错误码注册表设计如此）+ api.rs。
d08a=$(rg -n 'use crate::api' crates/hexagon-core/src \
       | rg -v 'src/(scenario|setup|replay)\.rs:')
d08b=$(rg -n 'crate::api::' crates/hexagon-core/src \
       | rg -v 'crate::api::Workbench' \
       | rg -v 'src/(scenario|setup|replay|errcode|api)\.rs:')
deny D08 "crate::api references stay in whitelist {scenario,setup,replay,errcode,api}" \
  "$(printf '%s\n%s' "$d08a" "$d08b" | rg -v '^\s*$')"

# D13 模型槽回退单点：get("default") 命中只许 provider_config.rs
# （resolve_slot 本体），其余必须挂 D13-exempt 行内理由。
deny D13 "get(\"default\") only in resolve_slot or D13-exempt" \
  "$(rg -n 'get\("default"\)' crates/hexagon-core/src \
     | rg -v 'provider_config\.rs:|D13-exempt')"

# D10 System 子 kind 词表：登记完整性由 trace.rs 测试
# system_subkinds_stay_registered 守（读回校验）。以下为仍写裸字面量
# 的写入点清单——提示级，常量化的重构面，不入禁线。
echo "── D10 (info): System subkind literals emitted inline（登记由测试守；常量化是重构面）"
rg -n 'json!\(\{"kind":' crates/hexagon-core/src \
  | rg -v 'tests\.rs|:[0-9]+:\s*//' | sed 's/^/    /'

if [ "$fail" -ne 0 ]; then
  echo "check-arch FAILED"; exit 1
fi
echo "check-arch ok — D01/D04/D09/D06/D08/D13 enforced, D10 informational"
