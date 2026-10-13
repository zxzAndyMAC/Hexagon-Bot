// 向导草稿（localStorage `hexagon.wizard`）：形状、读档迁移、项目身份键、角色定制规则。
// 从 Wizard.tsx 拆出（ui-redesign 票 10）：草稿规则是纯逻辑，组件只负责渲染与调用。
import type { PackDef, RoleDef } from '../../api'

export const DRAFT_KEY = 'hexagon.wizard'

export const STEPS = ['providers', 'dir', 'brief', 'roles', 'flow', 'keys', 'confirm'] as const
export type Step = (typeof STEPS)[number]

export interface Draft {
  dir: string
  name: string
  roles: string[]
  /// ADR 0057：按角色名存定制后的完整 RoleDef（只作用本项目，不回写模板库）。
  /// 缺项 = 用模板原定义。
  roleOverrides: Record<string, RoleDef>
  /// override 里哪些来自 AI 起草（可再起草覆盖）；人手编辑即移出此名单——
  /// 人手编辑永远不被 AI 盖掉。老草稿缺省为空，其中 AI 落的 override 视同人手（保守，宁不多盖）。
  roleDrafted: string[]
  /// 详情区发生过人工编辑的角色（ADR 0091：原为「定制卡点过保存」）。只有这份名单不参与 AI 起草。
  /// 不能用「有 override 但不在 roleDrafted」来推断——那份名单曾经只记下
  /// 产品策划，其余角色的旧稿就被永久跳过（owner 2026-09-25）。「恢复模板」清掉该标记。
  roleHandTuned: string[]
  /// 上次 AI 起草时用的项目身份（目录 + 名称 + 说明正文）。
  /// 和当前不一致则 AI 职责按模板显示，按钮回到「起草」。
  roleDraftKey?: string
  /// 目录里已有的说明文件正文。有它时起草以它为准，不用向导里留下的旧稿。
  instructionText: string
  mode: 'pack' | 'fastpath'
  packName: string
  fastRole: string
  initGit: boolean
  genAgents: boolean
  agentsMd: string
  /// 票 16：空目录项目说明的种子。优化前不落盘。启动页一句话写进这里（票 10）。
  brief: string
  /// 确认前的流程草稿。快速通道不采用它。
  flowPack: PackDef | null
  /// 起草这份流程时的项目身份。和当前不一致就重写，避免换项目后确认页还挂着上一个流程名。
  flowDraftKey: string | null
  fastPath: boolean
}

/// 启动页一句话开局带进向导的种子（interaction §11，WCAG 2.2 redundant-entry）。
export interface DraftSeed {
  brief: string
  dir?: string
}

// roles 步默认全选（owner 裁决 2026-09-25，取代票 08 的「只勾项目经理」）：
// 只作用全新草稿——存档草稿按保存的勾选恢复，卸掉的角色不被默认值重新勾上
// （票 08 语义不丢）。模板拉取失败时回落这里写死的「项目经理」。
export const EMPTY: Draft = {
  dir: '', name: '', roles: ['项目经理'], roleOverrides: {}, roleDrafted: [], roleHandTuned: [], instructionText: '', mode: 'pack',
  packName: '', fastRole: '', initGit: false, genAgents: false, agentsMd: '', brief: '',
  flowPack: null, flowDraftKey: null, fastPath: false,
}

export const dirName = (dir: string) => dir.split('/').filter(Boolean).pop() ?? ''

export function loadDraft(seed?: DraftSeed): Draft {
  let merged: Draft
  try {
    const stored = JSON.parse(localStorage.getItem(DRAFT_KEY) || '{}')
    merged = { ...EMPTY, ...stored }
    // roleDrafted 引入前的老草稿：键缺省说明 override 只能经旧版起草按钮
    // 或定制卡落库。缺省按「全部 AI 起草」回填——否则老草稿按钮永久锁死、
    // 无从再起草。代价：老草稿里真手改的 override 会被下次起草覆盖——
    // 结果仍落可编辑 override，损失有界（owner 裁决 2026-09-25）。
    if (stored.roleDrafted === undefined) {
      merged.roleDrafted = Object.keys(merged.roleOverrides ?? {})
    }
    // 老存档没有起草时的项目身份：用当时的说明钉住，说明一改就视为过期。
    if (merged.roleDraftKey == null && merged.roleDrafted.length > 0) {
      merged.roleDraftKey = projectKey(merged)
    }
  } catch {
    merged = { ...EMPTY }
  }
  if (!seed) return merged
  // 启动页带入：一句话覆盖 brief；选过目录才覆盖 dir，并用目录名补空的项目名。
  const brief = seed.brief.trim()
  const dir = seed.dir?.trim()
  return {
    ...merged,
    brief: brief || merged.brief,
    ...(dir ? { dir, name: merged.name || dirName(dir) } : {}),
  }
}

export function projectKey(d: Pick<Draft, 'dir' | 'name' | 'instructionText' | 'agentsMd' | 'brief'>) {
  // 一句话和生成稿都算项目身份。只盯其中一份时，改了另一份不会让旧职责过期。
  const text = d.instructionText.trim() || `${d.agentsMd.trim()}\0${d.brief.trim()}`
  return `${d.dir}\0${d.name}\0${text}`
}

// 2026-09-30 原生验收：角色变更必须使旧流程失效，不能复用全模板名单生成的草稿。
export function flowKey(d: Draft) {
  return JSON.stringify([projectKey(d), d.roles])
}

/// 起草用的说明。一句话改了而生成稿还是上一个项目时，以一句话为准，
/// 生成稿标成过期，避免模型只改第一个角色、其余照抄旧项目。
export function draftSource(d: Pick<Draft, 'instructionText' | 'agentsMd' | 'brief'>): string {
  const file = d.instructionText.trim()
  if (file) return file
  const md = d.agentsMd.trim()
  const brief = d.brief.trim()
  if (md && brief && !md.includes(brief)) {
    return `The owner's latest instruction replaces any older description:\n${brief}`
  }
  return md || brief
}

type RoleState = Pick<Draft, 'roleOverrides' | 'roleDrafted' | 'roleHandTuned'>

const sameDef = (a: RoleDef, b: RoleDef) => JSON.stringify(a) === JSON.stringify(b)

/// ADR 0091：详情区的人工编辑即改即生效，直接写草稿（创建项目前不落盘）。
/// 与模板同形不写 override——「已定制」只标真实差异；但人工编辑本身就让该角色
/// 转为人手（roleHandTuned），之后「AI 起草职责」不再覆盖它，哪怕改回了模板原样。
export function applyRoleEdit<T extends RoleState>(d: T, tpl: RoleDef, def: RoleDef): T {
  const overrides = { ...d.roleOverrides }
  if (sameDef(def, tpl)) delete overrides[tpl.name]
  else overrides[tpl.name] = def
  return {
    ...d,
    roleOverrides: overrides,
    roleDrafted: d.roleDrafted.filter((n) => n !== tpl.name),
    roleHandTuned: d.roleHandTuned.includes(tpl.name) ? d.roleHandTuned : [...d.roleHandTuned, tpl.name],
  }
}

/// ADR 0091「恢复模板」：把该角色还原成模板原样——清 override、清人手标记、清 AI 起草标记。
/// 撤销的是全部定制（含 AI 起草稿），之后「AI 起草职责」可以再起草它。
export function restoreRoleTemplate<T extends RoleState>(d: T, name: string): T {
  const overrides = { ...d.roleOverrides }
  delete overrides[name]
  return {
    ...d,
    roleOverrides: overrides,
    roleDrafted: d.roleDrafted.filter((n) => n !== name),
    roleHandTuned: d.roleHandTuned.filter((n) => n !== name),
  }
}
