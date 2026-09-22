import { describe, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import { EventRow } from './components/Timeline'
import type { TimelineItem } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const item = (payload: Record<string, unknown>): TimelineItem => ({
  event: {
    id: 1, project_id: 'p1', kind: 'pm_routed', agent_id: 'a0', stage_run_id: 'sr1',
    payload, created_at: '',
  },
  message: null,
})

async function textOf(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  const text = el.textContent ?? ''
  const route = el.querySelector('[data-route]')?.getAttribute('data-route')
  root.unmount()
  return { text, route }
}

describe('时间线标出派给了谁（票 08）', () => {
  it('派给角色、先不派活、花名册外拒绝各一行', async () => {
    await i18n.changeLanguage('zh-CN')
    const sent = await textOf(<EventRow item={item({ role: '后端', held: false, rejected: false })} />)
    expect(sent.text).toContain('派给 后端')
    expect(sent.route).toBe('后端')
    const held = await textOf(<EventRow item={item({ held: true, rejected: false, choice: '先不派活' })} />)
    expect(held.text).toContain('先不派活')
    expect(held.route).toBe('hold')
    const rejected = await textOf(<EventRow item={item({ held: false, rejected: true, raw: '架构师' })} />)
    expect(rejected.text).toContain('没有派活')
    expect(rejected.route).toBe('rejected')
  })
})
