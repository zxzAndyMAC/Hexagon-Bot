// ui-redesign 票 01：图标注册表换成 Lucide 后，任何语义名都不能渲染出空 svg
// （规范 foundations §7：语义名保留，字形换成 Lucide，描边 1.75）。
import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { ICON_NAMES, Icon } from './Icon'

describe('Icon registry (Lucide)', () => {
  it('keeps every legacy semantic name plus the redesign additions', () => {
    for (const n of ['computer', 'close', 'send', 'warn', 'help', 'hex', 'file-config', 'copy',
      'trash', 'inbox', 'layers', 'bot', 'chart-column'] as const) {
      expect(ICON_NAMES).toContain(n)
    }
  })

  it.each(ICON_NAMES)('%s renders a non-empty 1.75-stroke glyph', (name) => {
    const host = document.createElement('div')
    host.innerHTML = renderToStaticMarkup(<Icon name={name} />)
    const svg = host.querySelector('svg')!
    expect(svg).not.toBeNull()
    expect(svg.getAttribute('aria-hidden')).toBe('true')
    expect(svg.getAttribute('stroke-width')).toBe('1.75')
    expect(svg.getAttribute('width')).toBe('14')
    expect(svg.querySelectorAll('path, circle, rect, line, polyline, polygon, ellipse').length).toBeGreaterThan(0)
  })
})
