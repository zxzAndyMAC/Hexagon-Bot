// owner-followups-1015 #16：登记发布目标卡（components D8「登记发布目标」）——Agent 在聊天里拟的发布目标，
// 卡面列名称 / 命令 / 工作目录 / 凭据名；批准只认点「登记」，键盘批准被拦；驳回可走键位。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { handlePendingKey, kindTitleKey, severityOf } from './decisions'
import { isMac } from './keymap'
import { PendingCard } from './components/PendingCards'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const t = (k: string, o?: Record<string, unknown>) => i18n.t(k, o) as string
const card: PendingQuestion = {
  id: 'q12', kind: 'release_target', agent_id: 'a0', state: 'queued',
  payload: { name: 'npm', command: 'npm publish --access public', cwd: '', credentials: ['NPM_TOKEN'], proposed_by: 'a0' },
}
const approveKey = () => new KeyboardEvent('keydown', { key: 'Enter', metaKey: isMac, ctrlKey: !isMac, cancelable: true })

let el: HTMLDivElement
let root: Root
let saved: ReturnType<typeof useUiStore.getState>
beforeEach(async () => {
  await i18n.changeLanguage('en')
  saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench', team: [], artifacts: [], pending: [], tabs: [], activeTab: null, invalidate: async () => {} })
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => {
  await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks()
  useUiStore.setState(saved, true)
})

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

describe('register release target card', () => {
  it('shows the drafted target with credential names only and registers only by click', async () => {
    const approve = vi.spyOn(api, 'approveReleaseTargetCard').mockResolvedValue({ id: 'r1', name: 'npm', command: 'npm publish --access public', cwd: '', credentials: [{ name: 'NPM_TOKEN', set: false }] })
    await act(async () => root.render(<PendingCard q={card} top />))
    expect(el.textContent).toContain(t('cards.releaseTarget'))
    expect(el.textContent).toContain('npm publish --access public')
    expect(el.textContent).toContain('NPM_TOKEN')
    expect(el.textContent).toContain(t('release.cwdRoot'))
    expect(el.textContent).toContain(t('cards.releaseTargetHint'))
    expect(el.querySelector('input')).toBeNull()

    expect(await handlePendingKey(approveKey(), [card])).toBe('blocked-release')
    expect(approve).not.toHaveBeenCalled()

    const button = el.querySelector<HTMLButtonElement>('[data-release-target-approve]')!
    expect(button.textContent).toBe(t('cards.releaseTargetApprove'))
    await act(async () => button.click())
    await flush()
    expect(approve).toHaveBeenCalledExactlyOnceWith('q12')
  })

  it('declines through the card without registering', async () => {
    const reject = vi.spyOn(api, 'rejectReleaseTargetCard').mockResolvedValue(undefined)
    const approve = vi.spyOn(api, 'approveReleaseTargetCard')
    await act(async () => root.render(<PendingCard q={card} top />))
    const decline = [...el.querySelectorAll('button')].find((b) => b.textContent?.startsWith(t('cards.reject')))!
    await act(async () => decline.click())
    await flush()
    expect(reject).toHaveBeenCalledExactlyOnceWith('q12')
    expect(approve).not.toHaveBeenCalled()
  })

  it('ranks with the critical cards and has its own title', () => {
    expect(severityOf(card)).toBe(0)
    expect(kindTitleKey(card)).toBe('cards.releaseTarget')
  })

  it('reads in Chinese (non-English smoke)', async () => {
    await i18n.changeLanguage('zh-CN')
    await act(async () => root.render(<PendingCard q={card} top />))
    expect(el.textContent).toContain('登记发布目标')
    expect(el.querySelector('[data-release-target-approve]')?.textContent).toBe('登记')
    expect(el.textContent).toContain('设置·发布')
  })
})
