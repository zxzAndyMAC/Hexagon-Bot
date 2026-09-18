// Agent 确定色：agent id 哈希 → HSL。头像底色与用量折线共用，
// 保证同一 Agent 在时间线/团队/图表里颜色一致。

export function hashHue(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0
  return h % 360
}

export const agentColor = (agentId: string, light = 42) =>
  `hsl(${hashHue(agentId)}, 45%, ${light}%)`
