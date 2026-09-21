// AgentTab 步骤摘要整形（ui-audit 票 15 / P3）。
// 独立成文件：组件文件只导出组件（react/only-export-components 约束）。

// tool_called 的 payload.input 常为对象（fs_write 的 {path,content} 等）——
// String(obj) 渲染成 "[object Object]"。取 path 优先，否则截断序列化；标量直用。
export function toolInputSummary(p: Record<string, unknown>): string {
  if (p.path != null) return String(p.path)
  const inp = p.input
  if (inp == null) return ''
  if (typeof inp === 'object') {
    const o = inp as Record<string, unknown>
    if (o.path != null) return String(o.path)
    const s = JSON.stringify(inp)
    return s.length > 120 ? s.slice(0, 117) + '…' : s
  }
  return String(inp)
}
