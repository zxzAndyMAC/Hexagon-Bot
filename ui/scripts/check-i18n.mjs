#!/usr/bin/env node
// i18n 七语言 key 一致性守门（QA 委托种子 12 / GR-05 / TC-I-0201）：
// 2026-09 实测七文件 646 key 全等但无任何自动化，破窗只是时间问题。
// 本脚本把每个 locale 的嵌套对象拍平成点路径集合，与 en 基准对账——
// 缺 key（翻译漏加）或多 key（删 en 忘删译文）都拦下。
// 自证：I18N_LOCALES_DIR=<复制后删一 key 的目录> 应红。
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = process.env.I18N_LOCALES_DIR
  ?? join(dirname(fileURLToPath(import.meta.url)), '../src/i18n/locales')

const load = (f) => {
  const src = readFileSync(join(dir, f), 'utf8').replace(/^export default/, 'return')
  return new Function(src)() // locale 是纯字面量对象，无 import/类型语法
}
const flat = (o, p = '', out = new Set()) => {
  for (const [k, v] of Object.entries(o)) {
    const key = p ? `${p}.${k}` : k
    if (v && typeof v === 'object') flat(v, key, out)
    else out.add(key)
  }
  return out
}

const files = readdirSync(dir).filter((f) => f.endsWith('.ts')).sort()
const ref = flat(load('en.ts'))
let bad = 0
for (const f of files) {
  const keys = flat(load(f))
  const missing = [...ref].filter((k) => !keys.has(k))
  const extra = [...keys].filter((k) => !ref.has(k))
  if (f === 'en.ts' ? keys.size !== ref.size : missing.length || extra.length) {
    bad = 1
    console.error(`${f}: ${keys.size} keys — missing ${missing.length}, extra ${extra.length}`)
    for (const k of missing.slice(0, 10)) console.error(`  - ${k}`)
    for (const k of extra.slice(0, 10)) console.error(`  + ${k}`)
  } else console.log(`${f}: ${keys.size} keys — ok`)
}
if (bad) process.exit(1)
console.log(`i18n ok — ${files.length} locales, ${ref.size} keys each`)
