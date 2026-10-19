#!/usr/bin/env node
// 设计 token 守门（ui-audit 票 11 / P2-12）：--bd/--bg-0 这类失效变量名
// 曾静默透传 var() 落到控件边界消失。本脚本把 index.css 定义的变量集合
// 与 src 内全部 var(--x) 引用对账，引用集合 ⊆ 定义集合才放行。
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { checkContrast } from './contrast.mjs'

const ui = join(dirname(fileURLToPath(import.meta.url)), '..')
const css = readFileSync(join(ui, 'src/index.css'), 'utf8')
const defined = new Set([...css.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]))

// 票 15（ui-redesign 收缩）：Linear 派生旧 token 已从 index.css 删净，只认 foundations 的新 token。
// 旧名不只出现在 var() 里——monacoFile.ts 曾用 c('--text') 这类字面量读 :root，var() 对账抓不到，
// 静默落成透明色。所以旧名在任何引号或 var() 里出现都拒绝，index.css 重新定义旧名也拒绝。
const LEGACY = /(?<![\w-])--(legacy-[\w-]+|bg(?:-[123])?|text(?:-[23])?|border(?:-strong)?|(?:ok|err|flag|warn)(?:-soft|-border)?|on-accent|popover|r-md|r-lg|r-xl)(?![\w-])/g

const files = []
const walk = (d) => {
  for (const e of readdirSync(d)) {
    const p = join(d, e)
    if (statSync(p).isDirectory()) walk(p)
    else if (/\.(ts|tsx|css)$/.test(e)) files.push(p)
  }
}
walk(join(ui, 'src'))

const bad = []
for (const f of files) {
  const src = readFileSync(f, 'utf8')
  for (const m of src.matchAll(/var\((--[\w-]+)/g)) {
    if (!defined.has(m[1])) bad.push(`${f.replace(ui + '/', '')}: var(${m[1]})`)
  }
  // 注释里的旧名是出处说明（「旧 --err 迁到 --red」），不算引用
  const code = src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1')
  for (const m of code.matchAll(LEGACY)) bad.push(`${f.replace(ui + '/', '')}: legacy token ${m[0]} (foundations §10)`)
}
if (bad.length) {
  console.error('undeclared design tokens referenced:\n' + bad.map((b) => '  ' + b).join('\n'))
  process.exit(1)
}
// 票 09（owner-followups-1015）：对比度断言表——文字 token × 底 token × 阈值，深浅两套实算（表与算法见 contrast.mjs）
const weak = checkContrast(css)
if (weak.length) {
  console.error('contrast below WCAG AA (foundations「对比度」):\n' + weak.map((b) => '  ' + b).join('\n'))
  process.exit(1)
}
console.log(`tokens ok — ${defined.size} defined, all references resolve, contrast table passes (light + dark)`)
