// 首屏引导 FreshStart（ui-redesign 票 05，components C13、workbench#fresh）。
// 负责人还没发过第一条指令时，对话列顶部居中问候 + 开场草案卡 + 建议卡；
// 建议只填入输入条（fillComposer），不发送。开场分析的消息照常排在下面。
// gap B3（2026-10-15）：首张建议点名角色——取流程包第一阶段里第一个在团队中的角色
// （workbench#fresh「让产品策划写规格」）；票 05 时以「流程包角色不固定」为由不点名。
// 读不到流程包（快速通道 / 读取失败）或该阶段没有在队角色时回落不点名的旧文案。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { fillComposer, useFreshStartMarker } from '../composerFill'
import { mentionTokenText } from '../composerAtoms'
import type { PackDef } from '../gen/PackDef'
import { freshLead } from '../freshLead'
import { Icon, type IconName } from './Icon'
import { CardButton } from './ui/card'
import { IntakeCard } from './IntakeCard'

const SUGGESTIONS: { key: 'spec' | 'repo'; icon: IconName }[] = [
  { key: 'spec', icon: 'file-text' },
  { key: 'repo', icon: 'folder' },
]

export function FreshStart() {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const roles = team.length
  const stages = useUiStore((s) => s.stages.length)
  const [pack, setPack] = useState<PackDef | null>(null)
  useEffect(() => {
    let live = true
    api.currentProcessPack().then((p) => { if (live) setPack(p) }).catch(() => {})
    return () => { live = false }
  }, [])
  const lead = freshLead(pack, team)
  useFreshStartMarker()
  const copy = (key: 'spec' | 'repo', part: 'Title' | 'Fill') =>
    key === 'spec' && lead
      ? t(`fresh.spec${part}Role`, { role: lead.role, stage: lead.stage, mention: mentionTokenText(lead.role) })
      : t(`fresh.${key}${part}`)
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
            onClick={() => fillComposer(copy(s.key, 'Fill'))}>
            <span className="ic"><Icon name={s.icon} size={14} /></span>
            <span><b>{copy(s.key, 'Title')}</b><small>{t(`fresh.${s.key}Desc`)}</small></span>
          </CardButton>
        ))}
      </div>
    </div>
  )
}
