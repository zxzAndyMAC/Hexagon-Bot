import type { i18n as I18n } from 'i18next'

// ADR 0071：agent 的回复语言跟随界面语言。核心读宿主文件，所以启动时同步一次、
// 之后每次切换再同步。写失败不打断界面——核心回落英文回复。
export function syncUiLanguageToCore(i18n: I18n, push: (code: string) => Promise<void>): () => void {
  const send = (code: string) => {
    push(code).catch(() => {})
  }
  send(i18n.language)
  i18n.on('languageChanged', send)
  return () => i18n.off('languageChanged', send)
}
