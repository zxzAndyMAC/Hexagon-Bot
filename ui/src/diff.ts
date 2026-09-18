// 行级 diff：公共前后缀裁剪 + LCS DP。产物是文本规格/代码，
// 体量通常 <2000 行；超限退化为「中间整块变更」粗粒度结果。

export type DiffOp = { type: 'eq' | 'del' | 'ins'; text: string }

const MAX_DP_LINES = 1500

export function diffLines(a: string[], b: string[]): DiffOp[] {
  // 公共前缀/后缀裁剪
  let pre = 0
  while (pre < a.length && pre < b.length && a[pre] === b[pre]) pre++
  let suf = 0
  while (
    suf < a.length - pre && suf < b.length - pre &&
    a[a.length - 1 - suf] === b[b.length - 1 - suf]
  ) suf++
  const midA = a.slice(pre, a.length - suf)
  const midB = b.slice(pre, b.length - suf)

  const ops: DiffOp[] = []
  const push = (type: DiffOp['type'], text: string) => ops.push({ type, text })

  if (midA.length > MAX_DP_LINES || midB.length > MAX_DP_LINES) {
    a.slice(0, pre).forEach((l) => push('eq', l))
    midA.forEach((l) => push('del', l))
    midB.forEach((l) => push('ins', l))
    a.slice(a.length - suf).forEach((l) => push('eq', l))
    return ops
  }

  // LCS DP（Uint32 省内存）
  const n = midA.length
  const m = midB.length
  const dp = new Uint32Array((n + 1) * (m + 1))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i * (m + 1) + j] = midA[i] === midB[j]
        ? dp[(i + 1) * (m + 1) + j + 1] + 1
        : Math.max(dp[(i + 1) * (m + 1) + j], dp[i * (m + 1) + j + 1])
    }
  }
  // 回溯出 op 序列
  const mid: DiffOp[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (midA[i] === midB[j]) {
      mid.push({ type: 'eq', text: midA[i] }); i++; j++
    } else if (dp[(i + 1) * (m + 1) + j] >= dp[i * (m + 1) + j + 1]) {
      mid.push({ type: 'del', text: midA[i] }); i++
    } else {
      mid.push({ type: 'ins', text: midB[j] }); j++
    }
  }
  while (i < n) { mid.push({ type: 'del', text: midA[i] }); i++ }
  while (j < m) { mid.push({ type: 'ins', text: midB[j] }); j++ }

  a.slice(0, pre).forEach((l) => push('eq', l))
  mid.forEach((o) => ops.push(o))
  a.slice(a.length - suf).forEach((l) => push('eq', l))
  return ops
}

/** 解析统一 diff 文本为 op 序列（提案卡的 ```diff 块）。 */
export function parseUnifiedDiff(text: string): DiffOp[] {
  return text.split('\n').map((l): DiffOp => {
    if (l.startsWith('+')) return { type: 'ins', text: l.slice(1) }
    if (l.startsWith('-')) return { type: 'del', text: l.slice(1) }
    return { type: 'eq', text: l.replace(/^ /, '') }
  })
}

/** 从产物正文提取 ```diff ... ``` 块。 */
export function extractDiffBlock(body: string): string | null {
  const m = /```diff\s*\n([\s\S]*?)```/.exec(body)
  return m ? m[1].trimEnd() : null
}
