// owner-followups-1015 票 20：头像提案卡——每格「换一张 / 跳过」，底部「全部应用」，提示词折叠；
// 不是待决卡：不进待决坞、不经裁决回路，只有点「全部应用」才写头像。
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import { EventRow } from './components/Timeline'
import { buildRows } from './timelineModel'
import { api } from './api'
import type { TimelineItem } from './api'
import type { ArtistProposal } from './gen/ArtistProposal'
import { useUiStore } from './store'
import { usageActorName } from './usage'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const IMG = 'data:image/png;base64,AAAA'
const sys = (id: number, payload: Record<string, unknown>): TimelineItem => ({
  event: { id, project_id: 'p1', kind: 'system', agent_id: 'a0', stage_run_id: null, payload, created_at: '' },
  message: null,
})
const event = sys(9, { kind: 'artist_avatar_proposal', speaker: '画师', speaker_code: 'artist', proposal_id: 'ap1', style: 'pixel art', model: 'gpt-image-1', roles: ['后端', '前端', '测试'] })
const proposal = (over: Partial<ArtistProposal> = {}): ArtistProposal => ({
  id: 'ap1', style: 'pixel art', model: 'gpt-image-1', applied: [],
  items: [
    { role: '后端', prompt: 'pixel art. backend', image: IMG, failure: null },
    { role: '前端', prompt: 'pixel art. frontend', image: IMG, failure: null },
    { role: '测试', prompt: 'pixel art. qa', image: null, failure: 'provider_unavailable' },
  ],
  ...over,
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  await act(async () => {})
  return { el, unmount: () => { root.unmount(); el.remove() } }
}
const button = (el: Element, text: string) => [...el.querySelectorAll('button')].find((b) => b.textContent === text)!

afterEach(() => vi.restoreAllMocks())

describe('头像提案卡（票 20）', () => {
  it('每格角色名 + 图；失败格写明没生成；提示词折叠', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.spyOn(api, 'artistProposal').mockResolvedValue(proposal())
    const { el, unmount } = await render(<EventRow item={event} />)
    const card = el.querySelector('[data-artist-proposal="ap1"]')!
    expect(card.querySelector('.tl-who b')!.textContent).toBe('画师')
    expect(card.textContent).toContain('头像提案')
    expect(card.querySelectorAll('.artist-cell')).toHaveLength(3)
    expect(card.querySelector('[data-role="后端"] img')!.getAttribute('src')).toBe(IMG)
    expect(card.querySelector('[data-role="测试"]')!.textContent).toContain('这张没有生成出来')
    const details = card.querySelector('details.artist-prompts')!
    expect(details.hasAttribute('open')).toBe(false)
    expect(details.textContent).toContain('pixel art. frontend')
    // 不是待决卡：独立形卡（不在坞里），没有裁决键位提示。
    expect(card.querySelector('.dc')).not.toBeNull()
    expect(card.querySelector('kbd')).toBeNull()
    unmount()
  })

  it('跳过的格不应用；全部应用只发有图且未跳过的角色，之后显示已应用', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.spyOn(api, 'artistProposal').mockResolvedValue(proposal())
    const apply = vi.spyOn(api, 'artistApply').mockResolvedValue(proposal({ applied: ['后端'] }))
    const invalidate = vi.fn(async () => {})
    const original = useUiStore.getState().invalidate
    useUiStore.setState({ invalidate })
    const pendingBefore = useUiStore.getState().pending
    const { el, unmount } = await render(<EventRow item={event} />)
    const front = el.querySelector('[data-role="前端"]')!
    await act(async () => { button(front, '跳过').click() })
    expect(front.hasAttribute('data-skipped')).toBe(true)
    expect(button(front, '取消跳过').getAttribute('aria-pressed')).toBe('true')
    // 失败格不能跳过（本来就不会应用）。
    expect(button(el.querySelector('[data-role="测试"]')!, '跳过').disabled).toBe(true)
    await act(async () => { button(el, '全部应用').click() })
    expect(apply).toHaveBeenCalledWith('ap1', ['后端'])
    expect(invalidate).toHaveBeenCalledWith('team')
    expect(el.textContent).toContain('已应用到 1 个角色')
    expect(button(el, '全部应用')).toBeUndefined()
    expect(useUiStore.getState().pending).toBe(pendingBefore)
    unmount()
    useUiStore.setState({ invalidate: original })
  })

  it('换一张只重画那一格', async () => {
    await i18n.changeLanguage('en')
    vi.spyOn(api, 'artistProposal').mockResolvedValue(proposal())
    const fresh = 'data:image/png;base64,BBBB'
    const regen = vi.spyOn(api, 'artistRegenerate').mockResolvedValue(proposal({
      items: proposal().items.map((it) => it.role === '测试' ? { ...it, image: fresh, failure: null } : it),
    }))
    const { el, unmount } = await render(<EventRow item={event} />)
    await act(async () => { button(el.querySelector('[data-role="测试"]')!, 'Try another').click() })
    expect(regen).toHaveBeenCalledWith('ap1', '测试')
    expect(el.querySelector('[data-role="测试"] img')!.getAttribute('src')).toBe(fresh)
    unmount()
    await i18n.changeLanguage('zh-CN')
  })

  it('不折进系统事件组；用量里画师按名字显示', () => {
    const rows = buildRows([
      sys(1, { kind: 'known_world' }),
      sys(2, { kind: 'artist_image_request' }),
      event,
      sys(6, { kind: 'known_world' }),
    ], 'all')
    expect(rows.some((r) => r.type === 'item' && r.item.event.payload.kind === 'artist_avatar_proposal')).toBe(true)
    expect(usageActorName([{ id: 'a0', role: '后端' }], 'artist')).toBe('画师')
    expect(usageActorName([{ id: 'a0', role: '后端' }], 'a0')).toBe('后端')
  })
})
