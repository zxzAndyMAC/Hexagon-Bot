// 产物在盘上的布局——UI 里唯一一处（ticket 33 S12，2026-10-25 批次审查）。
// 出处：core `artifacts::content` / `artifacts::deliver` 把产物写在项目根下 `.hexagon/<登记 path>`，
// 登记的 path 是 `.hexagon/` 内相对（`specs/prd.md`）。界面上凡要在「登记路径」与「仓内路径」之间换算的
// （文件菜单、页签复制路径 / 打开方式、文件页的版本链基线、dev mock）都走这里，不再各自拼 `.hexagon/` 字面量。
// 修复即文档（#32 2026-10-25）：#29 / #31 的产物页签直接用登记 path，指到项目根下同名路径（不存在）。

/** 产物目录（项目相对，带尾斜杠）。 */
export const ARTIFACT_DIR = '.hexagon/'

/** 产物登记路径 → 仓内路径（`.hexagon/<path>`）。绝对形 / 已带产物目录前缀的原样返回。 */
export function artifactRepoPath(path: string): string {
  if (path.startsWith('/') || path.startsWith(ARTIFACT_DIR)) return path
  return `${ARTIFACT_DIR}${path.replace(/^\.\//, '')}`
}

/** 仓内路径 → 产物登记路径；不在产物目录下 → null。 */
export function artifactRegisteredPath(repoPath: string): string | null {
  return repoPath.startsWith(ARTIFACT_DIR) ? repoPath.slice(ARTIFACT_DIR.length) : null
}
