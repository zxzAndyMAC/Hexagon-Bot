// 设置「权限」里的审查者模式行（ui-redesign 票 11 原「自治」分区；R-07 平铺 E1 行 + Switch）。
// 2026-10-26 负责人裁决：自治不再分档（ADR 0069）后「自治」页只剩这一个开关，独占一个导航项不值，
// 并入权限页（审查者能否拦截产物属于放权边界）。被否决：直接删除——会失去切换 shadow / live 的唯一入口。
// 开 = live（可拦截），切到 live 要二次确认；
// 确认层暂用现有 askConfirm，票 13 统一换成 AlertDialog。宿主确认前开关不乐观翻转（A10 加载行）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../../api'
import { useUiStore } from '../../store'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Group, SRow } from './kit'
import { useBusyAction } from '../../useBusyAction'

export function ReviewerModeRow() {
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
  )
}
