// 电脑动作确认卡（components D8「电脑动作」，control#action；票 ui-redesign 16B）的「动作」行调用式。
// 只用 computer_action 入参里宿主收到的字段，不推断目标含义。

function num(v: unknown): string {
  return typeof v === 'number' && Number.isFinite(v) ? String(Math.round(v)) : '?'
}

/** 动作调用式：click(412, 288) / type("…") / key(cmd,a) / scroll(down, 3) / drag(1, 2 → 3, 4) / activate()。 */
export function computerActionCall(input: Record<string, unknown>): string {
  const op = String(input.op ?? '')
  switch (op) {
    case 'click': {
      const kind = input.click_type && input.click_type !== 'single' ? `, ${String(input.click_type)}` : ''
      return `click(${num(input.x)}, ${num(input.y)}${kind})`
    }
    case 'type': {
      const text = String(input.text ?? '')
      return `type(${JSON.stringify(text.length > 60 ? `${text.slice(0, 60)}…` : text)})`
    }
    case 'key': return `key(${String(input.chord ?? '')})`
    case 'scroll': return `scroll(${String(input.direction ?? '')}, ${num(input.amount ?? 1)})`
    case 'drag': return `drag(${num(input.x)}, ${num(input.y)} → ${num(input.to_x)}, ${num(input.to_y)})`
    case 'activate': return 'activate()'
    default: return op
  }
}
