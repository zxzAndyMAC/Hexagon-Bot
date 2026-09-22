// AgentTab 步骤摘要整形（ui-audit 票 15 / P3）。
// 独立成文件：组件文件只导出组件（react/only-export-components 约束）。
// beautiful-ui 票 02：TOOL_LABEL/TOOL_ICON/pairToolCalls 升为 AgentTab +
// Timeline ToolChips 共用层。

import type { TimelineItem } from './api'
import type { IconName } from './components/Icon'

// tool_called 的 payload.input 常为对象（fs_write 的 {path,content} 等）——
// String(obj) 渲染成 "[object Object]"。取 path 优先，否则截断序列化；标量直用。
export function toolInputSummary(p: Record<string, unknown>): string {
  if (p.path != null) return String(p.path)
  const inp = p.input
  if (inp == null) return ''
  if (typeof inp === 'object') {
    const o = inp as Record<string, unknown>
    if (o.path != null) return String(o.path)
    // bash 的 cmd 直接取——JSON 序列化会把命令裹成 {"cmd":"…"} 噪声。
    if (o.cmd != null) return String(o.cmd)
    const s = JSON.stringify(inp)
    return s.length > 120 ? s.slice(0, 117) + '…' : s
  }
  return String(inp)
}

// 工具定名/图标词表。双写两族名：mock 数据用点号（fs.read），真事件走下划线
// （fs_read/fs_patch/artifact_write，core tools/builtin.rs 定名）。未登记工具
// 回退 stepTool/'tool' 图标 + 原始名（不吞新工具）。
export const TOOL_LABEL: Record<string, string> = {
  'fs.read': 'stepRead', 'fs.write': 'stepWrite', 'fs.edit': 'stepWrite',
  'fs.list': 'stepList', 'fs.search': 'stepSearch', bash: 'stepBash',
  fs_read: 'stepRead', fs_write: 'stepWrite', fs_patch: 'stepWrite',
  fs_list: 'stepList', fs_search: 'stepSearch',
  artifact_write: 'stepWrite',
  bash_output: 'stepBash', bash_kill: 'stepBash',
}

export const TOOL_ICON: Record<string, IconName> = {
  'fs.read': 'artifact', 'fs.write': 'diff', 'fs.edit': 'diff',
  'fs.list': 'folder', 'fs.search': 'list', bash: 'tool',
  fs_read: 'artifact', fs_write: 'diff', fs_patch: 'diff',
  fs_list: 'folder', fs_search: 'list',
  artifact_write: 'artifact',
  bash_output: 'tool', bash_kill: 'tool',
}

// 一次调用对 = tool_called 吸收紧随的 tool_result（AgentTab 步骤折叠同约定）。
// 孤儿 tool_result（无 called 先行）不计入——缺上下文渲染不出有意义的行。
// exec-cards 票 03 补 agent 校验：并发回合的事件在流里交错，别家的
// result 不能吸进本家调用（否则本家 called 永挂运行环、显示别家输出）。
export type ToolCall = { called: TimelineItem; result?: TimelineItem }

export function pairToolCalls(items: TimelineItem[]): ToolCall[] {
  const out: ToolCall[] = []
  for (let i = 0; i < items.length; i++) {
    const it = items[i]
    if (it.event.kind !== 'tool_called') continue
    const nx = items[i + 1]
    const res = nx?.event.kind === 'tool_result' && nx.event.agent_id === it.event.agent_id
      ? items[++i]
      : undefined
    out.push({ called: it, result: res })
  }
  return out
}

// 文件片收集：scrub 后的 input 只剩 {path[,bytes]}（safety.rs），顶层 path
// 是 mock/旧形状兼容位。无 path 的工具调用不出文件片。
export function toolFilePath(p: Record<string, unknown>): string | null {
  if (p.path != null) return String(p.path)
  const inp = p.input
  if (inp && typeof inp === 'object' && (inp as Record<string, unknown>).path != null) {
    return String((inp as Record<string, unknown>).path)
  }
  return null
}

// exec-cards 票 02（spec D2 分层）：升执行卡的重负载工具——命令/写系
// 出 ExecCard，读系（fs_read/fs_list/fs_search/bash_output/bash_kill）
// 保持 chip 行。chip 管扫读、卡管细看。
export const EXEC_CARD_TOOLS = new Set(['bash', 'fs_patch', 'fs_write', 'artifact_write'])
