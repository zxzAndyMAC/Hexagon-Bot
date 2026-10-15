// 切换项目阻塞横条（ui-redesign 票 13，feedback#errors 末行，interaction §13）：
// 头部下方 warn 横条：alert 图标（--orange）+「切换项目需要等待：<项目> 还有电脑动作没有排空…」+ Button xs「留在此项目」。
// --orange-tint 底、圆角 12、padding 10 12、13px；随对话列宽（--col-w）居中。
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor, formatBinding, matches } from '../keymap'
import { Icon } from './Icon'
import { Button } from '@/components/ui/button'
import { stayInProject, useSwitchWait } from '../projectSwitch'

export function SwitchWaitBanner() {
  const { t } = useTranslation()
  const wait = useSwitchWait((s) => s.wait)
  useEffect(() => {
    if (!wait) return
    const onKey = (e: KeyboardEvent) => {
      const b = bindingFor('stayInProject')
      if (b && matches(e, b)) { e.preventDefault(); stayInProject() }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [wait])
  if (!wait) return null
  const binding = bindingFor('stayInProject')
  return (
    <div className="mx-auto w-full max-w-[var(--col-w)] px-6 pt-2" data-switch-wait>
      <div role="status" className="flex items-center gap-2 rounded-[12px] bg-orange-tint px-3 py-2.5 text-[13px] text-ink">
        <Icon name="warn" size={16} className="shrink-0 text-orange" />
        <span className="min-w-0 flex-1">
          <b className="font-semibold">{t('feedback.switchWaitTitle')}</b>{t('feedback.switchWaitBody', { project: wait.project })}
        </span>
        <Button size="xs" title={binding ? `${t('feedback.stayHere')} ${formatBinding(binding)}` : t('feedback.stayHere')} onClick={stayInProject}>{t('feedback.stayHere')}</Button>
      </div>
    </div>
  )
}
