// 中栏内嵌 Monaco（ADR 0031 / 票 11）。只挂编辑器本体：
// 不注册调试器、不装插件。语法色用内置 monarch，worker 只服务编辑器内核。
// 主题不走 vs/vs-dark 原厂皮：编辑器壳色全部从 :root 的 CSS 变量取，
// data-theme 三态切换时重算（MutationObserver），与 index.css 同一套 token。
import * as monaco from 'monaco-editor'
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import 'monaco-editor/min/vs/editor/editor.main.css'

let envReady = false
let themeObs: MutationObserver | null = null

function ensureEnv() {
  if (envReady) return
  envReady = true
  // 内置语言服务的 worker，不是插件、也不是调试器（ADR 0031）。
  ;(globalThis as unknown as { MonacoEnvironment?: { getWorker: (id: string, label: string) => Worker } }).MonacoEnvironment = {
    getWorker: (_id, label) => {
      if (label === 'json') return new jsonWorker()
      if (label === 'css' || label === 'scss' || label === 'less') return new cssWorker()
      if (label === 'html' || label === 'handlebars' || label === 'razor') return new htmlWorker()
      if (label === 'typescript' || label === 'javascript') return new tsWorker()
      return new editorWorker()
    },
  }
}

/** CSS 变量值 → RGBA。只认本仓 token 的两种写法：#hex 与 rgb()/rgba()。 */
function parseColor(raw: string): { r: number; g: number; b: number; a: number } | null {
  const s = raw.trim()
  const hex = s.match(/^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i)
  if (hex) {
    let h = hex[1]
    if (h.length <= 4) h = h.split('').map((c) => c + c).join('')
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
      a: h.length >= 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1,
    }
  }
  const fn = s.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/i)
  if (fn) return { r: +fn[1], g: +fn[2], b: +fn[3], a: fn[4] != null ? +fn[4] : 1 }
  return null
}

const hex2 = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0')

/** 读一个 :root 变量，alpha 参数对 token 自带透明度做乘法缩放。 */
function cssColor(cs: CSSStyleDeclaration, name: string, alpha = 1): string {
  const p = parseColor(cs.getPropertyValue(name))
  if (!p) return '#00000000'
  return `#${hex2(p.r)}${hex2(p.g)}${hex2(p.b)}${hex2(p.a * alpha * 255)}`
}

/** 把产品 token 灌进 Monaco 主题并重设全局主题。分裂视图多个编辑器共享主题，
 *  defineTheme/setTheme 都是全局级，切主题只需重定义同名 'hexagon'。 */
function syncTheme() {
  const cs = getComputedStyle(document.documentElement)
  const c = (name: string, alpha = 1) => cssColor(cs, name, alpha)
  const dark = document.documentElement.dataset.theme !== 'light'
  monaco.editor.defineTheme('hexagon', {
    base: dark ? 'vs-dark' : 'vs',
    inherit: true,
    rules: [
      // 语法 token 沿用基底 monarch 配色，只把注释压到产品的第三级灰。
      { token: 'comment', foreground: c('--text-3').slice(1, 7) },
    ],
    colors: {
      'editor.background': c('--bg'),
      'editor.foreground': c('--text'),
      'editorGutter.background': c('--bg'),
      'editorLineNumber.foreground': c('--text-3', 0.8),
      'editorLineNumber.activeForeground': c('--text-2'),
      'editorCursor.foreground': c('--accent'),
      'editor.selectionBackground': c('--accent', 0.32),
      'editor.inactiveSelectionBackground': c('--accent', 0.16),
      'editor.selectionHighlightBackground': c('--accent', 0.14),
      'editor.wordHighlightBackground': c('--accent', 0.14),
      'editor.findMatchHighlightBackground': c('--accent', 0.3),
      'editor.lineHighlightBackground': c('--bg-2'),
      'editor.lineHighlightBorder': '#00000000',
      'editorIndentGuide.background1': c('--border'),
      'editorIndentGuide.activeBackground1': c('--border-strong'),
      'editorWhitespace.foreground': c('--border-strong'),
      'editorBracketMatch.background': c('--accent-soft'),
      'editorBracketMatch.border': c('--accent-border'),
      'editorBracketHighlight.foreground1': c('--accent'),
      'editorBracketHighlight.foreground2': c('--ok'),
      'editorBracketHighlight.foreground3': c('--warn'),
      'editorWidget.background': c('--popover'),
      'editorWidget.border': c('--border-strong'),
      'editorSuggestWidget.background': c('--popover'),
      'editorSuggestWidget.border': c('--border-strong'),
      'editorSuggestWidget.selectedBackground': c('--accent-soft'),
      'editorSuggestWidget.selectedForeground': c('--text'),
      'editorHoverWidget.background': c('--popover'),
      'editorHoverWidget.border': c('--border-strong'),
      'input.background': c('--bg'),
      'input.border': c('--border-strong'),
      'inputOption.activeBorder': c('--accent'),
      'focusBorder': c('--accent'),
      'scrollbarSlider.background': c('--border-strong', 0.6),
      'scrollbarSlider.hoverBackground': c('--text-3', 0.6),
      'scrollbarSlider.activeBackground': c('--text-2', 0.6),
      'editorError.foreground': c('--err'),
      'editorWarning.foreground': c('--warn'),
    },
  })
  monaco.editor.setTheme('hexagon')
}

function ensureThemeSync() {
  syncTheme()
  if (!themeObs) {
    themeObs = new MutationObserver(() => syncTheme())
    themeObs.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
  }
}

/** 产品等宽字体栈：monaco 会拿这个串做字形测量，传字面值不能传 var()。 */
function monoFont(): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue('--mono').trim()
  return v || 'ui-monospace, SFMono-Regular, Menlo, monospace'
}

function langOf(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    mjs: 'javascript', cjs: 'javascript', rs: 'rust', py: 'python',
    json: 'json', css: 'css', html: 'html', htm: 'html', md: 'markdown',
    sh: 'shell', bash: 'shell', yaml: 'yaml', yml: 'yaml',
  }
  return map[ext] ?? 'plaintext'
}

export function mountFileEditor(
  el: HTMLElement,
  path: string,
  value: string,
  onChange: (next: string) => void,
): { dispose: () => void } {
  ensureEnv()
  ensureThemeSync()
  const ed = monaco.editor.create(el, {
    value,
    language: langOf(path),
    theme: 'hexagon',
    minimap: { enabled: false },
    automaticLayout: true,
    fontFamily: monoFont(),
    fontSize: 12,
    lineHeight: 19,
    padding: { top: 8, bottom: 8 },
    scrollBeyondLastLine: false,
    wordWrap: 'on',
    glyphMargin: false,
    folding: true,
    overviewRulerLanes: 0,
    renderLineHighlight: 'line',
    scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10 },
    tabSize: 2,
    fixedOverflowWidgets: true,
  })
  const sub = ed.onDidChangeModelContent(() => onChange(ed.getValue()))
  return {
    dispose: () => {
      sub.dispose()
      ed.dispose()
    },
  }
}
