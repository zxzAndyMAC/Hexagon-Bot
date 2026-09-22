import type { IconName } from './Icon'

export type TreeKind = 'file' | 'dir' | 'link'

const CODE = new Set(['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'rs', 'py', 'go', 'css', 'html', 'htm', 'sh', 'bash'])
const TEXT = new Set(['md', 'txt', 'rst'])
const JSONISH = new Set(['json', 'jsonc'])
const IMAGE = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico'])
const CONFIG = new Set(['yaml', 'yml', 'toml', 'ini', 'lock'])
const CONFIG_NAMES = new Set(['makefile', 'dockerfile', 'license', '.gitignore', '.gitattributes', '.editorconfig'])

/** 文件树类型图标。目录/链接与扩展名分家，避免清一色文件图标。 */
export function fileIcon(name: string, kind: TreeKind): IconName {
  if (kind === 'dir') return 'folder'
  if (kind === 'link') return 'link'
  const base = name.toLowerCase()
  if (CONFIG_NAMES.has(base) || base.startsWith('.env')) return base === 'license' ? 'file-text' : 'file-config'
  const ext = base.includes('.') ? base.slice(base.lastIndexOf('.') + 1) : ''
  if (JSONISH.has(ext)) return 'file-json'
  if (IMAGE.has(ext)) return 'file-image'
  if (TEXT.has(ext)) return 'file-text'
  if (CONFIG.has(ext)) return 'file-config'
  if (CODE.has(ext)) return 'file-code'
  return 'file'
}
