// owner-followups-1015 票 19：画师的「需要出图模型」提示——以画师的名字与头像发言，
// 按钮打开设置·模型服务并聚焦出图槽；不折进系统事件组。
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import { EventRow } from './components/Timeline'
import { ProviderManager } from './components/ProviderManager'
import { buildRows } from './timelineModel'
import { api } from './api'
import type { TimelineItem } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const sys = (id: number, payload: Record<string, unknown>): TimelineItem => ({
  event: { id, project_id: 'p1', kind: 'system', agent_id: 'a0', stage_run_id: null, payload, created_at: '' },
  message: null,
})
const notice = (state: 'unconfigured' | 'unsupported', model?: string) => sys(5, {
  kind: 'artist_image_model_missing', speaker: '画师', speaker_code: 'artist', slot: 'image', state, model: model ?? null,
  style: 'pixel art', roles: ['后端'],
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, unmount: () => { root.unmount(); el.remove() } }
}

afterEach(() => vi.restoreAllMocks())

describe('画师提示（票 19）', () => {
  it('以画师的名字与头像说明需要出图模型', async () => {
    await i18n.changeLanguage('zh-CN')
    const { el, unmount } = await render(<EventRow item={notice('unconfigured')} />)
    const row = el.querySelector('[data-artist-notice="unconfigured"]')!
    expect(row).not.toBeNull()
    expect(row.querySelector('.tl-who b')!.textContent).toBe('画师')
    expect(row.querySelector('[aria-label="画师"], [title="画师"]')).not.toBeNull()
    expect(row.textContent).toContain('出图模型')
    unmount()
  })

  it('绑了不支持出图的模型时点名那个模型', async () => {
    await i18n.changeLanguage('en')
    const { el, unmount } = await render(<EventRow item={notice('unsupported', 'gpt-5')} />)
    expect(el.querySelector('[data-artist-notice="unsupported"]')!.textContent).toContain('gpt-5')
    unmount()
    await i18n.changeLanguage('zh-CN')
  })

  it('按钮打开设置·模型服务，出图槽那一行拿到焦点', async () => {
    await i18n.changeLanguage('zh-CN')
    const sections: unknown[] = []
    const onSection = (e: Event) => sections.push((e as CustomEvent).detail)
    window.addEventListener('hexagon:open-settings-section', onSection)
    const { el, unmount } = await render(<EventRow item={notice('unconfigured')} />)
    const button = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('去设置绑定出图模型'))!
    await act(async () => { button.click() })
    window.removeEventListener('hexagon:open-settings-section', onSection)
    expect(sections).toEqual(['models'])
    unmount()

    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
    vi.spyOn(api, 'providerPresets').mockResolvedValue([])
    const settings = await render(<ProviderManager />)
    await act(async () => {})
    const imageRow = settings.el.querySelector('[data-slot-row="image"]')!
    expect(imageRow.contains(document.activeElement)).toBe(true)
    settings.unmount()

    // 只聚焦一次：之后正常打开设置不再抢焦点。
    const again = await render(<ProviderManager />)
    await act(async () => {})
    expect(again.el.querySelector('[data-slot-row="image"]')!.contains(document.activeElement)).toBe(false)
    again.unmount()
  })

  it('不折进系统事件组', () => {
    const rows = buildRows([
      sys(1, { kind: 'known_world' }),
      sys(2, { kind: 'request_envelope' }),
      notice('unconfigured'),
      sys(6, { kind: 'known_world' }),
    ], 'all')
    expect(rows.some((r) => r.type === 'item' && r.item.event.payload.kind === 'artist_image_model_missing')).toBe(true)
  })
})

// 票 28：画师的主对话模型写不出出图提示词时，同一形制说明；没有对话模型才给按钮（聚焦画师的主对话槽），
// 调用失败 / 输出不可用没有可去的设置，只说明。
const promptFailed = (reason: string) => sys(7, {
  kind: 'artist_prompt_failed', speaker: '画师', speaker_code: 'artist', reason, slot: 'chat',
  style: 'pixel art', roles: ['后端'],
})

describe('画师写不出出图提示词（票 28）', () => {
  it('没有对话模型：说明且按钮打开设置·模型服务', async () => {
    await i18n.changeLanguage('zh-CN')
    const sections: unknown[] = []
    const onSection = (e: Event) => sections.push((e as CustomEvent).detail)
    window.addEventListener('hexagon:open-settings-section', onSection)
    const { el, unmount } = await render(<EventRow item={promptFailed('chat_model_missing')} />)
    const row = el.querySelector('[data-artist-notice="prompt_no_model"]')!
    expect(row).not.toBeNull()
    expect(row.querySelector('.tl-who b')!.textContent).toBe('画师')
    const button = row.querySelector('button')!
    expect(button).not.toBeNull()
    await act(async () => { button.click() })
    window.removeEventListener('hexagon:open-settings-section', onSection)
    expect(sections).toEqual(['models'])
    unmount()
  })

  it('调用失败 / 输出不可用：只说明，不给按钮', async () => {
    await i18n.changeLanguage('zh-CN')
    for (const reason of ['chat_model_failed', 'invalid_prompts', 'budget_exhausted']) {
      const { el, unmount } = await render(<EventRow item={promptFailed(reason)} />)
      const row = el.querySelector('[data-artist-notice="prompt_failed"]')!
      expect(row).not.toBeNull()
      expect(row.querySelector('button')).toBeNull()
      expect(row.textContent).toContain(i18n.t('artist.promptFailed'))
      unmount()
    }
  })

  it('不折进系统事件组', () => {
    const rows = buildRows([
      sys(1, { kind: 'known_world' }),
      promptFailed('invalid_prompts'),
      sys(6, { kind: 'known_world' }),
    ], 'all')
    expect(rows.some((r) => r.type === 'item' && r.item.event.payload.kind === 'artist_prompt_failed')).toBe(true)
  })
})
