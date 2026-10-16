import type { ChartConfig } from './chart'

/**
 * 提示与图例都按 config 声明序排（Recharts 的 payload 顺序不可靠）。
 * 票 16B 原型逐节核对：票 12 曾倒排成「堆叠顶层在前」（输入 / 输出 / 工具输出；每 Agent 线按用量升序），
 * 与原型不符——usage#detail 的图例与 ctip 都是 工具输出 / 输出 / 输入（声明序，底层在前），
 * usage#agents 图例按用量降序（= 声明序）。改回声明序，调用方按原型的阅读顺序声明。
 */
export function byConfigOrder<T extends { dataKey?: unknown; value?: unknown; type?: string }>(payload: ReadonlyArray<T>, config: ChartConfig): T[] {
  const order = Object.keys(config)
  const rank = (p: T) => order.indexOf(String(p.dataKey ?? p.value ?? ""))
  return payload.filter((p) => p.type !== "none").sort((a, b) => rank(a) - rank(b))
}
