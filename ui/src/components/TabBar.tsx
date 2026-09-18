import { useTranslation } from 'react-i18next'
import { useUiStore, type WorkTab } from '../store'
import { bindingFor, formatBinding } from '../keymap'
import { Icon, type IconName } from './Icon'

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
  const splitTip = `${t('tabs.split')} ${formatBinding(bindingFor('splitEditor'))}`
  const closeTip = `${t('tabs.close')} ${formatBinding(bindingFor('closeTab'))}`

  return (
    <div className="row-line" style={{ display: 'flex', alignItems: 'center', padding: '0 6px', gap: 2, background: 'var(--bg-1)' }}>
      {tabs.map((tab) => {
        const active = tab.id === activeTab
        const title = tab.kind === 'timeline' ? t('tabs.timeline')
          : tab.kind === 'agent' ? (tab.role ?? tab.title)
          : tab.title
        return (
          <div
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            style={{
              display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px',
              fontSize: 12, cursor: 'pointer', userSelect: 'none',
              color: active ? 'var(--text)' : 'var(--text-3)',
              borderBottom: active ? '2px solid var(--accent)' : '2px solid transparent',
              fontWeight: active ? 560 : 400,
              maxWidth: 200,
            }}
          >
            <Icon name={KIND_ICON[tab.kind]} size={11} />
            <span className="mono" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: 11 }}>
              {title}
            </span>
            {tab.kind !== 'timeline' && (
              <button
                className="icon-btn"
                style={{ padding: '0 3px', display: 'inline-flex', alignItems: 'center' }}
                title={closeTip}
                onClick={(e) => { e.stopPropagation(); closeTab(tab.id) }}
              >
                <Icon name="close" size={10} />
              </button>
            )}
          </div>
        )
      })}
      <div style={{ flex: 1 }} />
      <button
        className={`icon-btn ${splitOpen ? 'accent' : ''}`}
        style={{ padding: '2px 8px', display: 'inline-flex', alignItems: 'center' }}
        title={splitTip}
        onClick={() => setSplitOpen(!splitOpen)}
      >
        <Icon name="split" size={13} />
      </button>
    </div>
  )
}
