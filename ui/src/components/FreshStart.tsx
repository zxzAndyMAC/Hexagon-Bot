// 首屏引导 FreshStart（ui-redesign 票 05，components C13、workbench#fresh）。
// 负责人还没发过第一条指令时，对话列顶部居中问候 + 开场草案卡 + 建议卡；
// 建议只填入输入条（fillComposer），不发送。开场分析的消息照常排在下面。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { fillComposer } from '../composerFill'
import { Icon, type IconName } from './Icon'
import { CardButton } from './ui/card'
import { IntakeCard } from './IntakeCard'

const SUGGESTIONS: { key: 'spec' | 'repo'; icon: IconName }[] = [
  { key: 'spec', icon: 'file-text' },
  { key: 'repo', icon: 'folder' },
]

export function FreshStart() {
  const { t } = useTranslation()
  const roles = useUiStore((s) => s.team.length)
  const stages = useUiStore((s) => s.stages.length)
  return (
    <div className="tl-fresh" data-fresh-start="">
      <div className="tl-fresh-hero">
        <div className="hexmark" aria-hidden><Icon name="hex" size={20} /></div>
        <h2>{t('fresh.greeting')}</h2>
        <p>{t('fresh.sub', { roles, stages })}</p>
      </div>
      <IntakeCard />
      <div className="tl-fresh-grid">
        {SUGGESTIONS.map((s) => (
          <CardButton key={s.key} data-fresh-suggestion={s.key} title={t('returnSummary.fillHint')}
            onClick={() => fillComposer(t(`fresh.${s.key}Fill`))}>
            <span className="ic"><Icon name={s.icon} size={14} /></span>
            <span><b>{t(`fresh.${s.key}Title`)}</b><small>{t(`fresh.${s.key}Desc`)}</small></span>
          </CardButton>
        ))}
      </div>
    </div>
  )
}
