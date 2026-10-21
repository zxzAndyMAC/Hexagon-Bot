// 右键菜单「打开方式 ▸」子菜单（components A16b，owner-followups-1015 #31）。
// 行 = 16px 系统应用图标（取不到用中性 external 图标 ink-3）+ 系统显示名（不翻译，过长省略）；
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

/** `path` 项目相对；空串 = 项目根（IDE 以文件夹加载整个项目，只列支持打开目录的应用）。 */
export function OpenWithSubmenu({ path }: { path: string }) {
  const { t } = useTranslation()
  const { state, retry } = useOpenWithCandidates(path)
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
          <ContextMenuItem key={app.id} data-open-with-app={app.id} icon={<AppIcon app={app} />} onSelect={() => void openPathWith(path, app.id)}>
            <span className="min-w-0 truncate" title={app.name}>{app.name}</span>
          </ContextMenuItem>
        ))}
      </ContextMenuSubContent>
    </ContextMenuSub>
  )
}
