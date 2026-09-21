#!/usr/bin/env node
// 设计 token 守门（ui-audit 票 11 / P2-12）：--bd/--bg-0 这类失效变量名
// 曾静默透传 var() 落到控件边界消失。本脚本把 index.css 定义的变量集合
// 与 src 内全部 var(--x) 引用对账，引用集合 ⊆ 定义集合才放行。
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ui = join(dirname(fileURLToPath(import.meta.url)), '..')
const css = readFileSync(join(ui, 'src/index.css'), 'utf8')
const defined = new Set([...css.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]))

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
}
if (bad.length) {
  console.error('undeclared design tokens referenced:\n' + bad.map((b) => '  ' + b).join('\n'))
  process.exit(1)
}
console.log(`tokens ok — ${defined.size} defined, all references resolve`)
