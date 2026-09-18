import { useEffect, useState } from 'react'
import { createHighlighter, type Highlighter } from 'shiki'

let hlPromise: Promise<Highlighter> | null = null

const LANGS = [
  'tsx', 'ts', 'jsx', 'js', 'rust', 'python', 'bash', 'json', 'css', 'html',
  'toml', 'yaml', 'markdown', 'diff', 'sql', 'text',
]

export function getHighlighter() {
  hlPromise ??= createHighlighter({
    themes: ['github-light', 'github-dark'],
    langs: LANGS,
  })
  return hlPromise
}

export function useTheme(): 'light' | 'dark' {
  const [theme, setTheme] = useState<'light' | 'dark'>(() =>
    document.documentElement.dataset.theme === 'light' ? 'light' : 'dark')
  useEffect(() => {
    const obs = new MutationObserver(() =>
      setTheme(document.documentElement.dataset.theme === 'light' ? 'light' : 'dark'))
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => obs.disconnect()
  }, [])
  return theme
}

export function langFor(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'ts', tsx: 'tsx', js: 'js', jsx: 'jsx', mjs: 'js', cjs: 'js',
    rs: 'rust', py: 'python', sh: 'bash', bash: 'bash', zsh: 'bash',
    json: 'json', css: 'css', html: 'html', htm: 'html',
    toml: 'toml', yaml: 'yaml', yml: 'yaml', md: 'markdown',
    sql: 'sql', diff: 'diff', patch: 'diff',
  }
  return map[ext] ?? 'text'
}
