// 建议弹层（ui-redesign 票 07，components D2 / A16，composer#mention · composer#path）。
// A16 菜单形，贴输入条上沿、宽同输入条；焦点留在 textarea，`aria-activedescendant` 指向高亮行。
// @：分组「点名角色」，行 = 20 头像 + 粗名（多实例带 [id] 等宽 ink-3）+ 灰职责（gap B7，TeamRow.duty）+ 右侧在岗绿点 / 「休眠」。
// #：行 = 目录 / 文件图标 + 路径等宽 + 右侧类型（「目录」/「产物 vN」）；底部「Type to search · 最多 60 条」。
// 加载：首屏 3 行骨架；防抖期间保持旧结果（调用方不清空 items）。错误：单行红字 + 「重试」。
import { forwardRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Avatar } from '@/components/ui/avatar'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import { Button } from '@/components/ui/button'

import type { SuggestPopup } from '../composerSuggest'

export const ComposerSuggestions = forwardRef<HTMLDivElement, {
  id: string
  popup: SuggestPopup
  sel: number
  onPick: (label: string) => void
  onHover: (index: number) => void
  onRetry: () => void
}>(function ComposerSuggestions({ id, popup, sel, onPick, onHover, onRetry }, ref) {
  const { t } = useTranslation()
  const avatars = useUiStore((s) => s.avatars)
  const mention = popup.kind === '@'
  const firstLoad = popup.loading && popup.items.length === 0
  return (
    <div ref={ref} id={id} className="composer-suggestions" role="listbox"
      aria-label={mention ? t('composer.mentionHint') : t('composer.pathHint')} aria-busy={popup.loading || undefined}>
      {mention && <div className="composer-pop-sec">{t('composer.mentionGroup')}</div>}
      {firstLoad && [0, 1, 2].map((n) => <div key={n} className="composer-pop-skel" aria-hidden><i style={{ width: `${70 - n * 15}%` }} /></div>)}
      {popup.error && !popup.items.length && (
        <div className="composer-pop-err" role="alert">
          <span style={{ flex: 1 }}>{t('composer.pathFailed')}</span>
          <Button size="xs" variant="ghost" onMouseDown={(e) => e.preventDefault()} onClick={onRetry}>{t('composer.retry')}</Button>
        </div>
      )}
      {popup.items.map((it, i) => (
        <div key={it.label} id={`${id}-${i}`} role="option" aria-selected={i === sel} data-label={it.label}
          className="composer-pop-row"
          onMouseDown={(event) => event.preventDefault()}
          onMouseEnter={() => onHover(i)}
          onClick={() => onPick(it.label)}>
          {it.kind === '@' ? <>
            <Avatar agentId={it.agentId} name={it.name} size={20} src={it.agentId ? avatars[it.agentId] : undefined} />
            <b>{it.name}{it.inst && <> <span className="inst">[{it.inst}]</span></>}</b>
            {it.duty && <span className="desc" title={it.duty}>{it.duty}</span>}
            <span className="d">{it.sleeping ? t('composer.sleeping') : <span className="on-dot" aria-label={t('composer.awake')} />}</span>
          </> : <>
            {it.dir ? <Icon name="folder" className="ic-dir" size={14} /> : <Icon name={it.artifact != null ? 'file-text' : 'file'} className="ic-file" size={14} />}
            <span className="mono">{it.path}</span>
            {(it.dir || it.artifact != null) && <span className="d">
              {it.dir ? t('composer.directory') : t('composer.artifactVersion', { version: it.artifact })}
            </span>}
          </>}
        </div>
      ))}
      <div className="composer-pop-sep" />
      <div className="composer-pop-foot">{mention ? t('composer.popKeys') : t('composer.pathFoot')}</div>
    </div>
  )
})
