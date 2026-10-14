// 用量详情主区视图（interaction §3 §10，usage#detail；ADR 0090）：占满主区，对话与输入条一起让出，侧栏仍在。
// 票 03 只把旧用量详情从中栏页签挪成主区视图，并给出「返回对话」；内容换皮在票 12。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { isMac } from '../../keymap'
import { UsageDetailTab } from '../UsageDetailTab'
import { Icon } from '../Icon'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export function UsageView({ narrowLead = false }: { narrowLead?: boolean }) {
  const { t } = useTranslation()
  const projectName = useUiStore((s) => s.projectName)
  return (
    <section data-usage-view aria-label={t('usage.detail')} className="flex min-h-0 min-w-0 flex-1 flex-col">
      <header data-tauri-drag-region className={cn('flex h-12 flex-none items-center gap-2 border-b border-line-soft pr-3 pl-4', isMac && narrowLead && 'pl-7')}>
        <Button variant="ghost" size="xs" data-usage-back onClick={() => useUiStore.getState().showConversation()}>
          <Icon name="arrow-left" size={14} />{t('canvas.backToChat')}
        </Button>
        <div data-tauri-drag-region className="flex min-w-0 items-center gap-1.5 text-[13px] text-ink-2">
          <b className="font-semibold text-ink">{t('usage.detail')}</b>
          {projectName && <span className="truncate text-ink-3">{projectName}</span>}
        </div>
      </header>
      <div className="flex min-h-0 flex-1 flex-col">
        <UsageDetailTab />
      </div>
    </section>
  )
}
