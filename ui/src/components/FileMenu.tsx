// 文件右键菜单（components A16b，owner-followups-1015 #32，2026-10-21 负责人裁决「凡是显示一个有盘上路径的文件的表面，
// 右键都给同一份文件菜单」）。文件树行（#07 / #31）与 #32 补齐的产物行、时间线文件引用、附件、执行卡路径等全部复用这里，
// 项序与文件树行一致：打开 · 插入到输入条 ·（分隔）复制路径（绝对）· 在访达中显示 · 打开方式 ▸。
// 不适用的项不渲染：没有「打开」入口的表面不给打开；不落盘（无路径）的对象没有路径类项与「打开方式」；
// 链接没有一条仓内路径可交给编辑器，不给「打开方式」（指向仓外的揭示由 core 拒）。一项都不剩 → 不挂菜单（右键无反应）。
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from '@/components/ui/context-menu'
import { copyProjectPath, insertPathToComposer, revealLabelKey, revealPath } from '../fileActions'
import { OpenWithSubmenu } from './OpenWithMenu'

export interface FileMenuTarget {
  /** 项目相对路径（空串 = 项目根）；null = 不落盘 / 不在项目内——不出路径类项与「打开方式」。 */
  path: string | null
  kind?: 'file' | 'dir' | 'link'
  /** 「打开」；不传 = 该表面没有打开动作，不显示。 */
  onOpen?: () => void
  /** 「插入到输入条」的路径（`treeInsertPath` 形）；不传 / null = 不显示。 */
  insert?: string | null
}

/** 菜单里一项都不剩？（调用方据此决定挂不挂菜单） */
export function fileMenuEmpty({ path, onOpen, insert }: FileMenuTarget): boolean {
  return path == null && !onOpen && !insert
}

/** 菜单项本体（放进任意 ContextMenuContent；消息菜单在 # 路径块上右键时切成这份）。 */
export function FileMenuItems({ path, kind = 'file', onOpen, insert }: FileMenuTarget) {
  const { t } = useTranslation()
  return (
    <>
      {onOpen && <ContextMenuItem data-ctx-item="open" onSelect={onOpen}>{t('ctx.open')}</ContextMenuItem>}
      {insert && (
        <ContextMenuItem data-ctx-item="insert" action="insertPath" onSelect={() => insertPathToComposer(insert)}>
          {t('ctx.insertToComposer')}
        </ContextMenuItem>
      )}
      {(onOpen || insert) && path != null && <ContextMenuSeparator />}
      {path != null && (
        <>
          <ContextMenuItem data-ctx-item="copyPath" action="copyPath" onSelect={() => void copyProjectPath(path)}>
            {t('ctx.copyPath')}
          </ContextMenuItem>
          <ContextMenuItem data-ctx-item="reveal" action="revealPath" onSelect={() => void revealPath(path)}>
            {t(revealLabelKey())}
          </ContextMenuItem>
          {kind !== 'link' && <OpenWithSubmenu path={path} />}
        </>
      )}
    </>
  )
}

/** 包一层右键菜单；`children` 必须是单个能接 ref 的元素（Trigger asChild）。一项都不剩时原样返回子元素。 */
export function FileContextMenu({ children, ...target }: FileMenuTarget & { children: ReactNode }) {
  if (fileMenuEmpty(target)) return <>{children}</>
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <FileMenuItems {...target} />
      </ContextMenuContent>
    </ContextMenu>
  )
}
