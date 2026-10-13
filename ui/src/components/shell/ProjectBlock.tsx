// 项目块 proj（components B1，workbench#running）：hexmark + 项目名 13.5/600 + 流程包副标 11.5 ink-3 + ▾，
// 打开项目菜单（A16）：最近项目切换、返回启动页。取代旧顶栏项目名下拉与流程包 chip（ADR 0088，interaction §1 去向表）。
// 切换 = 重载整个投影（ADR 0051）；已删目录（exists=false）不进切换菜单——清理入口在启动页。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { api, errText } from '../../api'
import type { RecentProject } from '../../gen/RecentProject'
import { Icon } from '../Icon'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

export function Hexmark() {
  return (
    <span aria-hidden className="grid size-7 flex-none place-items-center rounded-[8px] bg-ink text-page">
      <Icon name="hex" size={14} />
    </span>
  )
}

export function ProjectBlock({ onProjectClosed }: { onProjectClosed: () => void }) {
  const { t } = useTranslation()
  const projectName = useUiStore((s) => s.projectName)
  const packName = useUiStore((s) => s.packName)
  const mode = useUiStore((s) => s.mode)
  const [recents, setRecents] = useState<RecentProject[]>([])
  const name = projectName || t('app.untitledProject')
  const sub = mode === 'fastpath' ? t('sidebar.fastpath') : packName ? t('sidebar.pack', { name: packName }) : t('topbar.pack')

  const switchTo = async (dir: string) => {
    // TOCTOU：列出后目录被删仍会失败——toast 报出而不是静默拒绝
    try {
      await api.openRecent(dir)
      await useUiStore.getState().refresh()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    }
  }
  const backToLauncher = async () => {
    try {
      await api.closeProject()
      onProjectClosed()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    }
  }
  const others = recents.filter((r) => r.exists && r.name !== projectName)

  return (
    <DropdownMenu onOpenChange={(open) => { if (open) api.recentProjects().then(setRecents).catch(() => setRecents([])) }}>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          data-project-block
          aria-label={`${t('sidebar.projectMenu')} · ${name}`}
          className="hx-press hx-focus mx-2.5 mt-0.5 mb-2 flex w-[calc(100%-20px)] cursor-pointer items-center gap-2.5 rounded-[var(--r-card)] px-2 py-1.5 text-left live:hover:bg-hover"
        >
          <Hexmark />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-[13.5px] font-semibold text-ink">{name}</span>
            <span className="truncate text-[11.5px] text-ink-3">{sub}</span>
          </span>
          <Icon name="chevron-down" size={14} className="text-ink-3" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[240px]">
        {others.length > 0 && (
          <>
            <DropdownMenuLabel>{t('sidebar.recentProjects')}</DropdownMenuLabel>
            {others.map((r) => (
              <DropdownMenuItem key={r.dir} tall description={r.dir} onSelect={() => void switchTo(r.dir)}>
                {r.name}
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
          </>
        )}
        <DropdownMenuItem icon={<Icon name="arrow-left" />} onSelect={() => void backToLauncher()}>
          {t('sidebar.backToLauncher')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
