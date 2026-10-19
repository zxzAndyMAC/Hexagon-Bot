// 对比度断言（owner-followups-1015 票 09；规范 foundations「对比度」，2026-10-17 负责人裁决）。
// 起因：辅助文字 --ink-3、主按钮白字、语义胶囊字、侧栏休眠行在深浅两套下都低于 WCAG AA，暗色尤甚；
// token 是手调的 oklch，肉眼看不出差半档。所以 check-tokens 解析 index.css 两套主题，按
// 「文字 token × 底 token × 阈值」表实算对比度，有人改回不达标值即红。
//
// 计算口径：oklch → oklab → 线性 sRGB（逐通道夹到 [0,1]，即浏览器的 gamut clip 近似）→ WCAG 2.x 相对亮度。
// 带 alpha 的底（深色 --*-tint 是基色 /.14）按浏览器做法在 gamma 编码的 sRGB 里叠到它实际所在的底上。
// 选夹取而非 CSS Color 4 的色度映射：两者在本表的值上差 <0.05:1，夹取实现短、可读，阈值不靠这点余量。

const TAU = Math.PI / 180

/** 解析 `oklch(L% C H)` / `oklch(L% C H / A)`；L 也接受 0–1 小数。不是 oklch 返回 null。 */
export function parseOklch(value) {
  const m = /^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)\s*(?:\/\s*([\d.]+)(%?))?\s*\)$/.exec(value.trim())
  if (!m) return null
  const l = m[2] ? Number(m[1]) / 100 : Number(m[1])
  const a = m[5] === undefined ? 1 : m[6] ? Number(m[5]) / 100 : Number(m[5])
  return { l, c: Number(m[3]), h: Number(m[4]), a }
}

/** oklch → gamma 编码 sRGB（0–1，夹取），带 alpha。 */
export function oklchToSrgb({ l, c, h, a }) {
  const A = c * Math.cos(h * TAU)
  const B = c * Math.sin(h * TAU)
  const l_ = (l + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m_ = (l - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s_ = (l - 0.0894841775 * A - 1.291485548 * B) ** 3
  const lin = [
    4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
    -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
    -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_,
  ]
  const enc = (x) => {
    const v = Math.min(1, Math.max(0, x))
    return v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055
  }
  return { rgb: lin.map(enc), a }
}

/** 把带 alpha 的前景叠到不透明底上（gamma 空间，同浏览器合成）。 */
export function composite(fg, bg) {
  return { rgb: fg.rgb.map((v, i) => v * fg.a + bg.rgb[i] * (1 - fg.a)), a: 1 }
}

export function luminance({ rgb }) {
  const lin = rgb.map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]
}

export function contrastRatio(fg, bg) {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

/** 取出一个选择器块里的 `--x: value;` 声明。只认块首选择器文本完全相同的块（可多块合并）。 */
export function readThemeVars(css, selectorStart) {
  const vars = {}
  const re = /([^{}]+)\{([^{}]*)\}/g
  for (const m of css.matchAll(re)) {
    const sel = m[1].replace(/\/\*[\s\S]*?\*\//g, '').trim()
    if (!sel.startsWith(selectorStart)) continue
    const body = m[2].replace(/\/\*[\s\S]*?\*\//g, '')
    for (const d of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) vars[d[1]] = d[2].trim()
  }
  return vars
}

/** 解析颜色值：oklch 字面量、`white`、或 `var(--x)` 链。解析不了抛错（表里写错 token 也要红）。 */
export function resolveColor(name, vars, seen = new Set()) {
  if (name === 'white') return { rgb: [1, 1, 1], a: 1 }
  const raw = name.startsWith('--') ? vars[name] : name
  if (raw === undefined) throw new Error(`token ${name} 未定义`)
  const ref = /^var\((--[\w-]+)\)$/.exec(raw)
  if (ref) {
    if (seen.has(ref[1])) throw new Error(`token ${name} 循环引用`)
    seen.add(ref[1])
    return resolveColor(ref[1], vars, seen)
  }
  if (raw === 'white') return { rgb: [1, 1, 1], a: 1 }
  const p = parseOklch(raw)
  if (!p) throw new Error(`token ${name} 的值「${raw}」不是 oklch`)
  return oklchToSrgb(p)
}

/**
 * 断言表的一行：fg 文字 token，bg 底 token（可带 alpha），over 是 bg 半透明时它实际叠在哪个底上
 * （缺省 --surface：胶囊多落在卡片面上；深色 --surface 比 --canvas 亮，是淡底叠出来最亮、对比最差的一种），
 * min 阈值，themes 缺省 = 两套都查。
 * 返回每行实测值与是否达标。
 */
export function evaluate(vars, rows) {
  return rows.map((r) => {
    const base = resolveColor(r.over ?? '--surface', vars)
    let bg = resolveColor(r.bg, vars)
    if (bg.a < 1) bg = composite(bg, base)
    let fg = resolveColor(r.fg, vars)
    if (fg.a < 1) fg = composite(fg, bg)
    const ratio = contrastRatio(fg, bg)
    return { ...r, ratio, ok: ratio >= r.min }
  })
}

const AA = 4.5

const SEMANTIC = ['green', 'orange', 'red', 'violet']

/**
 * 「文字 token × 底 token × 阈值」表（深浅两套共用一张，token 值各取各的）。
 * 覆盖面按规范 foundations「对比度」：辅助文字 --ink-3 落在三种主要底上；主按钮白字；
 * 语义 -ink 落在胶囊淡底（叠在卡片面上）与卡片面本身（行内语义文字）；--accent-ink 同理。
 * 豁免（不进表）：分割线、装饰线、禁用控件；基色 --green 等只给点 / 图标 / 填充，图标的 3:1 不在本表。
 */
export const CONTRAST_TABLE = [
  { fg: '--ink-3', bg: '--surface', min: AA },
  { fg: '--ink-3', bg: '--page', min: AA },
  // 浅色 --canvas 不是 --ink-3 的主要底（浅色主区与卡片 = --surface、侧栏 = --page）；把它也拉进来要 L≈.54，
  // 与 --ink-2（.506）挤成一档，规范裁决「刚好达标、--ink-2 不动」取 L≈.56。深色主区就是 --canvas，必须进表。
  { fg: '--ink-3', bg: '--canvas', min: AA, themes: ['dark'] },
  { fg: 'white', bg: '--accent-strong', min: AA },
  { fg: '--accent-ink', bg: '--accent-tint', min: AA },
  { fg: '--accent-ink', bg: '--surface', min: AA },
  ...SEMANTIC.flatMap((c) => [
    { fg: `--${c}-ink`, bg: `--${c}-tint`, min: AA },
    { fg: `--${c}-ink`, bg: '--surface', min: AA },
    { fg: `--${c}-ink`, bg: '--canvas', min: AA },
  ]),
]

export const THEMES = [
  { name: 'light', selector: ':root,' },
  { name: 'dark', selector: "[data-theme='dark']," },
]

/** 对 index.css 全文跑整张表；返回不达标行的人读描述（空数组 = 通过）。 */
export function checkContrast(css, rows = CONTRAST_TABLE) {
  const light = readThemeVars(css, THEMES[0].selector)
  const bad = []
  for (const t of THEMES) {
    // 深色块只写差异，其余继承 :root（同浏览器层叠）
    const vars = t.name === 'light' ? light : { ...light, ...readThemeVars(css, t.selector) }
    for (const r of evaluate(vars, rows.filter((row) => !row.themes || row.themes.includes(t.name)))) {
      if (!r.ok) bad.push(`${t.name}: ${r.fg} on ${r.bg} = ${r.ratio.toFixed(2)}:1 < ${r.min}:1`)
    }
  }
  return bad
}
