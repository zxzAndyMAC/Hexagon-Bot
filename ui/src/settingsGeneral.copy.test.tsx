// 票 16B（settings#general）：通用分区行文照原型——「界面语言」「浅色 / 深色 / 跟随系统」「数据去向」「模型凭据」。
// 此前是「语言」「亮色 / 暗色」「数据存储与接收方」「凭据存储」。
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './api'
import i18n from './i18n'
import { SettingsPage } from './components/SettingsPage'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const el = document.createElement('div')
let root: ReturnType<typeof createRoot> | undefined

beforeEach(async () => { await i18n.changeLanguage('zh-CN'); document.body.appendChild(el) })
afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  root = undefined; el.replaceChildren(); el.remove(); vi.restoreAllMocks()
  await i18n.changeLanguage('en')
})

it('general section uses the prototype labels', async () => {
  vi.spyOn(api, 'dataBoundary').mockResolvedValue({ credential_backend: 'keychain', project_open: false, recipients: [] })
  root = createRoot(el)
  await act(async () => root!.render(<SettingsPage projectless onBack={() => {}} />))
  const text = el.textContent!
  for (const label of ['界面语言', '浅色', '深色', '跟随系统', '数据去向', '模型凭据']) expect(text).toContain(label)
  expect(text).not.toContain('数据存储与接收方')
})
