import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore, type WorkTab } from '../store'
import { bindingFor, formatBinding } from '../keymap'
import { Icon, type IconName } from './Icon'
import { Row } from './Row'

// ui-audit 票 11（P2-17）：分栏在 <720px 窗口下两半都不可读——禁用并说明。
const SPLIT_MIN_WIDTH = 720

const KIND_ICON: Record<WorkTab['kind'], IconName> = {
  timeline: 'list',
  artifact: 'artifact',
  diff: 'diff',
  agent: 'agent',
  usage: 'usage',
}

export function TabBar() {
  const { t } = useTranslation()
  const { tabs, activeTab, splitOpen, setActiveTab, closeTab, setSplitOpen } = useUiStore()
  const [narrow, setNarrow] = useState(() => window.innerWidth < SPLIT_MIN_WIDTH)
  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < SPLIT_MIN_WIDTH)
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])
  const splitTip = narrow
    ? t('tabs.splitNeedsWidth')
    : `${t('tabs.split')} ${formatBinding(bindingFor('splitEditor'))}`
  const closeTip = `${t('tabs.close')} ${formatBinding(bindingFor('closeTab'))}`

  const stripRef = useRef<HTMLDivElement>(null)
  const [overflow, setOverflow] = useState(false)
  const [moreOpen, setMoreOpen] = useState(false)

  useLayoutEffect(() => {
    const el = stripRef.current
    if (!el) return
    const check = () => setOverflow(el.scrollWidth > el.clientWidth + 1)
    check()
    const ro = new ResizeObserver(check)
    ro.observe(el)
    return () => ro.disconnect()
  }, [tabs])

  const title = (tab: WorkTab) =>
    tab.kind === 'timeline' ? t('tabs.timeline')
    : tab.kind === 'agent' ? (tab.role ?? tab.title)
    : tab.title

  return (
    <div className="row-line" style={{ display: 'flex', alignItems: 'center', padding: '0 6px', gap: 2, background: 'var(--bg-1)' }}>
      <div ref={stripRef} role="tablist" style={{ flex: 1, minWidth: 0, display: 'flex', alignItems: 'center', gap: 2, overflow: 'hidden' }}>
        {tabs.map((tab) => {
          const active = tab.id === activeTab
          return (
            // ui-audit 票 12（P2-13）：tab 语义 + 键盘可达（Row 原语）
            <Row
              key={tab.id}
              role="tab"
              selected={active}
              onClick={() => setActiveTab(tab.id)}
              // 票 15（P3）：中键关闭（时间线主 tab 除外——它没有 × 钮）
              onAuxClick={(e) => {
                if (e.button === 1 && tab.kind !== 'timeline') { e.preventDefault(); closeTab(tab.id) }
              }}
              style={{
                display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px',
                fontSize: 12, cursor: 'pointer', userSelect: 'none',
                color: active ? 'var(--text)' : 'var(--text-3)',
                borderBottom: active ? '2px solid var(--accent)' : '2px solid transparent',
                fontWeight: active ? 560 : 400,
                flex: '0 1 auto', minWidth: 88, maxWidth: 200,
              }}
            >
              <Icon name={KIND_ICON[tab.kind]} size={11} />
              <span className="mono" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: 11 }}>
                {title(tab)}
              </span>
              {tab.kind !== 'timeline' && (
                <button
                  className="icon-btn"
                  style={{ padding: '0 3px', display: 'inline-flex', alignItems: 'center', flexShrink: 0 }}
                  title={closeTip}
                  onClick={(e) => { e.stopPropagation(); closeTab(tab.id) }}
                >
                  <Icon name="close" size={10} />
                </button>
              )}
            </Row>
          )
        })}
      </div>
      {overflow && (
        <div style={{ position: 'relative', flexShrink: 0 }}>
          <button
            className={`icon-btn ${moreOpen ? 'accent' : ''}`}
            style={{ padding: '2px 6px', display: 'inline-flex', alignItems: 'center' }}
            title={t('tabs.more')}
            onClick={() => setMoreOpen(!moreOpen)}
          >
            <Icon name="chevron-down" size={11} />
          </button>
          {moreOpen && (
            <>
              <div style={{ position: 'fixed', inset: 0, zIndex: 40 }} onClick={() => setMoreOpen(false)} />
              <div className="panel" role="listbox" style={{
                position: 'absolute', right: 0, top: '100%', zIndex: 41, minWidth: 200,
                background: 'var(--popover)', boxShadow: '0 8px 24px rgba(0,0,0,.28)', overflow: 'hidden',
              }}>
                {tabs.map((tab) => (
                  <Row
                    key={tab.id}
                    role="option"
                    selected={tab.id === activeTab}
                    className="row-line"
                    style={{
                      display: 'flex', alignItems: 'center', gap: 8, padding: '6px 10px',
                      cursor: 'pointer', fontSize: 12,
                      color: tab.id === activeTab ? 'var(--text)' : 'var(--text-2)',
                      fontWeight: tab.id === activeTab ? 560 : 400,
                    }}
                    onClick={() => { setActiveTab(tab.id); setMoreOpen(false) }}
                  >
                    <Icon name={KIND_ICON[tab.kind]} size={11} />
                    <span className="mono" style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: 11 }}>
                      {title(tab)}
                    </span>
                    {tab.id === activeTab && <Icon name="check" size={10} />}
                  </Row>
                ))}
              </div>
            </>
          )}
        </div>
      )}
      <button
        className={`icon-btn ${splitOpen ? 'accent' : ''}`}
        style={{ padding: '2px 8px', display: 'inline-flex', alignItems: 'center', flexShrink: 0, opacity: narrow ? 0.4 : 1 }}
        title={splitTip}
        disabled={narrow}
        onClick={() => setSplitOpen(!splitOpen)}
      >
        <Icon name="split" size={13} />
      </button>
    </div>
  )
}
