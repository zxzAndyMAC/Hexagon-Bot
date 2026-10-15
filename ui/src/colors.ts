// Agent 确定色：agent id 哈希 → HSL。头像底色与用量折线共用，
// 保证同一 Agent 在时间线/团队/图表里颜色一致。

export function hashHue(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0
  // 黄金角扩散：a0/a1 这类相邻 id 的哈希只差 1，直接 %360 全撞色；
  // ×137.508° 后相邻输入错开约三分之一色环。
  return (h * 137.508) % 360
}

export const agentColor = (agentId: string, light = 42) =>
  `hsl(${hashHue(agentId)}, 45%, ${light}%)`

/** 规范 foundations §1.5：新头像只用 c1–c8 八色（`--agent-c{n}`），按同一哈希扩散取号。
 *  旧 agentColor 留给旧界面与用量折线，票 15 收缩时一并迁走。 */
export const agentSwatch = (agentId: string): number =>
  (Math.floor(hashHue(agentId) / 45) % 8) + 1

/** 图表里的 Agent 确定色（票 12：消费榜单条 / 每 Agent 最近使用的线）：c1–c8 写成字面量，token 守门能对账。 */
const AGENT_VARS = [
  'var(--agent-c1)', 'var(--agent-c2)', 'var(--agent-c3)', 'var(--agent-c4)',
  'var(--agent-c5)', 'var(--agent-c6)', 'var(--agent-c7)', 'var(--agent-c8)',
] as const
export const agentVar = (agentId: string): string => AGENT_VARS[agentSwatch(agentId) - 1]
