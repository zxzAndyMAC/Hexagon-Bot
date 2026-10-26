// 文件菜单的目标计算（components A16b，owner-followups-1015 #32）：从 FileMenu.tsx 拆出——组件文件只导出组件（fast refresh）。
import { artifactRepoPath, projectRelPath } from './fileActions'
import { treeInsertPath } from './composerAtoms'
import { useUiStore } from './store'

export interface FileMenuTarget {
  /** 项目相对路径（空串 = 项目根）；null = 不落盘 / 不在项目内——不出路径类项与「打开方式」。 */
  path: string | null
  kind?: 'file' | 'dir' | 'link'
  /** 「打开」；不传 = 该表面没有打开动作，不显示。 */
  onOpen?: () => void
  /** 「插入到输入条」的路径（`treeInsertPath` 形）；不传 / null = 不显示。 */
  insert?: string | null
}

/**
 * 表面上显示的一条文件路径（相对或绝对）→ 菜单目标：在项目根内才给路径类项 / 打开方式 / 插入（#32）。
 * `onOpen` 原样透传（「打开」不依赖路径在根内——例如产物页签按记录打开）。
 * `artifact` = `raw` 是产物登记路径（`.hexagon/` 内相对），盘上路径换成 `.hexagon/<raw>`。
 */
export function useProjectFileTarget(raw: string | null | undefined, opts: { onOpen?: () => void; kind?: 'file' | 'dir'; artifact?: boolean } = {}): FileMenuTarget {
  const root = useUiStore((s) => s.projectRoot)
  const kind = opts.kind ?? 'file'
  const path = raw ? projectRelPath(opts.artifact ? artifactRepoPath(raw) : raw, root) : null
  return { path, kind, onOpen: opts.onOpen, insert: path ? treeInsertPath({ path, kind }) : null }
}

/** 菜单里一项都不剩？（调用方据此决定挂不挂菜单） */
export function fileMenuEmpty({ path, onOpen, insert }: FileMenuTarget): boolean {
  return path == null && !onOpen && !insert
}
