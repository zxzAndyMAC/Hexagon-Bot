// 命令面板匹配/排序纯函数（ui-audit 票 09 / P1-5+P3-19）：
// keywords 参与匹配（英文动词/缩写，如 rew→退回上一阶段）；
// 空查询时最近使用项置顶（recents=localStorage 中的 id 序，新→旧）。
export interface PaletteItemLike {
  id: string
  label: string
  keywords?: string[]
}

export function filterPaletteItems<T extends PaletteItemLike>(
  items: T[],
  q: string,
  recents: string[],
): T[] {
  const needle = q.trim().toLowerCase()
  const rank = (i: T) => {
    const r = recents.indexOf(i.id)
    return r === -1 ? Number.MAX_SAFE_INTEGER : r
  }
  const rows = needle
    ? items.filter(
        (i) =>
          i.label.toLowerCase().includes(needle) ||
          (i.keywords ?? []).some((k) => k.toLowerCase().includes(needle)),
      )
    : items
  // 稳定排序：同 rank 保持声明序；非空查询不置顶（按匹配定）。
  return [...rows]
    .map((i, idx) => ({ i, idx }))
    .sort((a, b) => rank(a.i) - rank(b.i) || a.idx - b.idx)
    .map((x) => x.i)
}

export function pushRecent(recents: string[], id: string, cap = 8): string[] {
  return [id, ...recents.filter((x) => x !== id)].slice(0, cap)
}
