// 侧栏成员行的动作（components A16b 第一批「成员行」，owner-followups-1015 #08）。
// 右键菜单、命令面板、keymap 三个入口共用这里——右键不是唯一入口（A16b「定位」）：
// 「打开详情」同点击成员行；「@ 点名」同输入条 @ 弹层的点名块；「复制名字」写角色名。
// 「换头像 / 恢复默认」（#18）语义同设置·团队 / 向导的角色详情：改的是**角色层**（该角色全部实例生效），
// 不是画布 Agent 详情的实例覆盖——有实例覆盖的行换完看不到变化，这是规范 E4 的层级定义，不是 bug。
// 提示语随层级说清（#33 P5，2026-10-25 批次审查）：原先禁用理由写「没有自定义头像」，可该行明明显示着一张自定义图
// （实例覆盖），读起来自相矛盾。改为「没有角色头像」，并在有实例覆盖的行换 / 恢复后补一条提示「改的是角色层、
// 本行仍显示实例头像」。被否决：让菜单作用到可见的那张（实例覆盖）——会让同名菜单项在不同行改不同层，与 #18
// 「语义同角色详情」冲突；实例层的入口仍是 Agent 详情。
import i18n from './i18n'
import { api } from './api'
import { pickAvatarFile } from './components/AvatarPicker'
import { insertComposerMention } from './composerFill'
import { mentionName, mentionTokenText } from './composerAtoms'
import { useUiStore } from './store'
import { toastError } from './toast'

const memberOf = (id: string) => useUiStore.getState().team.find((m) => m.id === id)

/** 焦点（或给定节点）所在的成员行 → 成员 id；不在成员行上为 null。键位与命令面板据此找目标。 */
export function memberRowTarget(node: Element | null = document.activeElement): string | null {
  const row = node?.closest<HTMLElement>('[data-member]')
  return row?.dataset.member && memberOf(row.dataset.member) ? row.dataset.member : null
}

/** 打开详情：在画布打开该 Agent（同点击成员行）。 */
export function openMemberDetail(id: string) {
  const m = memberOf(id)
  if (m) useUiStore.getState().openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role })
}

/**
 * @ 点名：把点名块插进输入条并聚焦——只填不发。同名角色写成 `角色[id]`（reliability 07：只写角色名会把两个实例都派给第一个）。
 * 聚焦放到下一帧：菜单 / 命令面板关闭时会把焦点还给触发处，同步聚焦会被盖掉（调用方的菜单跳过还焦点）。
 */
export function mentionMember(id: string) {
  const s = useUiStore.getState()
  const m = memberOf(id)
  if (!m) return
  s.showConversation()
  insertComposerMention(mentionTokenText(mentionName(m, s.team)))
  requestAnimationFrame(() => document.getElementById('composer-input')?.focus())
}

/** 复制名字：写角色名；成功 toast「已复制名字」，失败走 toastError（A16b 错误态：菜单已关）。 */
export async function copyMemberName(id: string) {
  const m = memberOf(id)
  if (!m) return
  try {
    await navigator.clipboard.writeText(m.role)
    useUiStore.getState().pushToast(i18n.t('ctxMember.nameCopied'), 'ok')
  } catch (e) {
    toastError(e)
  }
}

/** 换头像：选图后写角色头像（同角色详情 `setRoleAvatar`），再刷新团队让各行重拉生效图。 */
export function changeMemberAvatar(id: string) {
  const m = memberOf(id)
  if (!m) return
  pickAvatarFile(async (url) => {
    await api.setRoleAvatar(m.role, url)
    await useUiStore.getState().invalidate('team')
    noteInstanceOverride(id)
  })
}

/** 角色层改完、但该行显示的是实例覆盖（看不到变化）时说一声；没有覆盖不打扰。 */
function noteInstanceOverride(id: string) {
  if (memberOf(id)?.avatar_override) useUiStore.getState().pushToast(i18n.t('ctxMember.instanceAvatarShown'), 'info')
}

/** 该成员的角色有没有自定义头像（「恢复默认」可用的条件）。读失败按「没有」处理：禁用比误删便宜。 */
export async function memberHasRoleAvatar(id: string): Promise<boolean> {
  const m = memberOf(id)
  if (!m) return false
  try { return (await api.roleAvatar(m.role)) != null } catch { return false }
}

/**
 * 恢复默认：删角色头像（同角色详情 `clearRoleAvatar`）→ 色块首字（有实例覆盖的行仍显示覆盖图）。
 * 键位 / 命令面板没有菜单的禁用态可看，没有可恢复的就出提示，不发删除。
 */
export async function restoreMemberAvatar(id: string) {
  const m = memberOf(id)
  if (!m) return
  const s = useUiStore.getState()
  if (!(await memberHasRoleAvatar(id))) { s.pushToast(i18n.t('ctxMember.noRoleAvatar'), 'info'); return }
  try {
    await api.clearRoleAvatar(m.role)
    await useUiStore.getState().invalidate('team')
    noteInstanceOverride(id)
  } catch (e) {
    toastError(e)
  }
}
