// 设置「自治」分区（ui-redesign 票 11；R-07：原型导航缺失，照现实现保留——平铺 E1 行 + Switch）。
// 自治不再分档（ADR 0069），这里只留审查者 shadow / live。开 = live（可拦截），切到 live 要二次确认；
// 确认层暂用现有 askConfirm，票 13 统一换成 AlertDialog。宿主确认前开关不乐观翻转（A10 加载行）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../../api'
import { useUiStore } from '../../store'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Group, SetHead, SetWrap, SRow } from './kit'
import { useBusyAction } from '../../useBusyAction'

export function AutonomySection() {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const askConfirm = useUiStore((s) => s.askConfirm)
  const [reviewer, setReviewer] = useState<string | null>(null)
  const { busy, run } = useBusyAction()

  useEffect(() => {
    api.reviewerMode().then(setReviewer).catch((e) => pushToast(errText(e), 'err'))
  }, [pushToast])

  const apply = (mode: string) => run(async () => {
    await api.setReviewerMode(mode)
    setReviewer(mode)
  })

  const setMode = (mode: 'shadow' | 'live') => {
    if (mode === reviewer) return
    if (mode === 'live') {
      askConfirm({ title: t('auto.liveTitle'), body: t('auto.liveBody'), danger: true, confirmLabel: t('auto.liveConfirm'), run: () => apply(mode) })
    } else {
      void apply(mode)
    }
  }

  const live = reviewer === 'live'
  return (
    <SetWrap>
      <SetHead title={t('settings.nav_autonomy')} desc={t('auto.floorsHint')} />
      <Group>
        <SRow label={t('auto.reviewer')} hint={t('auto.reviewerHint')}>
          {reviewer && <Badge variant={live ? 'info' : 'neutral'}>{t(`auto.mode_${live ? 'live' : 'shadow'}`)}</Badge>}
          <Switch
            aria-label={t('auto.mode_live')}
            checked={live}
            disabled={reviewer === null}
            loading={busy}
            onCheckedChange={(on) => setMode(on ? 'live' : 'shadow')}
          />
        </SRow>
      </Group>
    </SetWrap>
  )
}
