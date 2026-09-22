// 中栏内嵌 Monaco（ADR 0031 / 票 11）。只挂编辑器本体：
// 不注册调试器、不装插件。语法色用内置 monarch，worker 只服务编辑器内核。
import * as monaco from 'monaco-editor'
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import 'monaco-editor/min/vs/editor/editor.main.css'

let envReady = false

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
  const theme = document.documentElement.dataset.theme === 'light' ? 'vs' : 'vs-dark'
  const ed = monaco.editor.create(el, {
    value,
    language: langOf(path),
    theme,
    minimap: { enabled: false },
    automaticLayout: true,
    fontSize: 13,
    scrollBeyondLastLine: false,
    wordWrap: 'on',
    glyphMargin: false,
    folding: true,
    overviewRulerLanes: 0,
    renderLineHighlight: 'line',
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
