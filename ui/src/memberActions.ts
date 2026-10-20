// 侧栏成员行的动作（components A16b 第一批「成员行」，owner-followups-1015 #08）。
// 右键菜单、命令面板、keymap 三个入口共用这里——右键不是唯一入口（A16b「定位」）：
// 「打开详情」同点击成员行；「@ 点名」同输入条 @ 弹层的点名块；「复制名字」写角色名。
import i18n from './i18n'
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
