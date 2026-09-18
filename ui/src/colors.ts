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
