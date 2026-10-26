// 右键菜单「打开方式 ▸」子菜单（components A16b，owner-followups-1015 #31）。
// 行 = 16px 系统应用图标（取不到用中性 external 图标 ink-3）+ 系统显示名（不翻译，过长省略）
// + 系统默认应用名后 --ink-3「（默认）」（#32，名省略时标记不省略）；
// 读取中 = 3 行骨架（A16 加载态）；空 = 一行禁用「未找到可用的编辑器」；失败 = 一行 --red 文案 + 右侧「重试」（A16 错误态）。
// 键盘：→ 展开、← 收起、Esc 关闭（radix Sub）。动作 openWith 默认空绑定，触发行右侧显示当前绑定。
import { useTranslation } from 'react-i18next'
import { ContextMenuItem, ContextMenuSub, ContextMenuSubContent, ContextMenuSubTrigger } from '@/components/ui/context-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { Icon } from './Icon'
import { openPathWith, useOpenWithCandidates, type OpenWithApp } from '../openWith'

/** 系统应用图标 16px；取不到用中性图标（不按名称套品牌）。装饰性，旁边总有应用名。 */
export function AppIcon({ app }: { app: OpenWithApp }) {
  return app.icon
    ? <img src={app.icon} alt="" aria-hidden width={16} height={16} className="size-4 shrink-0" draggable={false} />
    : <Icon name="external" size={16} className="text-ink-3" />
}

/**
 * `path` 项目相对；空串 = 项目根（IDE 以文件夹加载整个项目，只列支持打开目录的应用）。
 * `recent` = 启动页最近项目（#32）：`path` 是 recents 记录里的目录，根约束由 core 按记录判。
 * 候选随文件类型变（#32，2026-10-21 负责人裁决）：非文本类给系统列表，系统默认应用在第一行、名后加「（默认）」。
 */
export function OpenWithSubmenu({ path, recent = false }: { path: string; recent?: boolean }) {
  const { t } = useTranslation()
  const { state, retry } = useOpenWithCandidates(path, recent)
  return (
    <ContextMenuSub>
      <ContextMenuSubTrigger data-ctx-item="openWith" action="openWith">{t('ctx.openWith')}</ContextMenuSubTrigger>
      <ContextMenuSubContent data-open-with>
        {state.kind === 'loading' && (
          <div role="status" aria-busy aria-label={t('ctx.openWithLoading')}>
            {[60, 44, 52].map((w) => (
              <div key={w} className="flex h-8 items-center gap-[9px] px-[9px]">
                <Skeleton className="size-4 shrink-0" />
                <Skeleton style={{ width: `${w}%` }} />
              </div>
            ))}
          </div>
        )}
        {state.kind === 'error' && (
          <ContextMenuItem data-open-with-error className="text-red-ink" onSelect={(e) => { e.preventDefault(); retry() }}>
            {t('ctx.openWithError')}
            <span className="ml-auto pl-3 text-[12px] text-ink-3">{t('ctx.retry')}</span>
          </ContextMenuItem>
        )}
        {state.kind === 'ready' && state.apps.length === 0 && (
          <ContextMenuItem data-open-with-empty disabled>{t('ctx.openWithEmpty')}</ContextMenuItem>
        )}
        {state.kind === 'ready' && state.apps.map((app) => (
          <ContextMenuItem key={app.id} data-open-with-app={app.id} icon={<AppIcon app={app} />} onSelect={() => void openPathWith(path, app.id, recent)}>
            <span className="min-w-0 truncate" title={app.name}>{app.name}</span>
            {app.is_default && <span data-open-with-default className="shrink-0 text-ink-3">{t('ctx.openWithDefault')}</span>}
          </ContextMenuItem>
        ))}
      </ContextMenuSubContent>
    </ContextMenuSub>
  )
}
