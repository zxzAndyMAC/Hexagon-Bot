// 角色直选模型（ui-audit-2 票 01 / report A4 彻底方案的落地实现）。
// 专属模型复用既有槽位解析链：把「供应商+模型」绑到 agent:<id> 命名槽、
// agent.model_slot 指向它——provider_config::resolve_slot（全仓唯一回退链，
// arch-review 票 01/D13）原样工作，绑定被删时自动回落 default 槽。
// 否决的替代方案：agents 表加 provider_id/model 列 + resolve_agent_model——
// 等价效果但要动迁移/解析链/update_agent 三处接缝，零收益。
import type { ProvidersView } from './gen/ProvidersView'
import type { SlotBinding } from './gen/SlotBinding'

export const DEDICATED_PREFIX = 'agent:'
export const dedicatedSlot = (agentId: string) => `${DEDICATED_PREFIX}${agentId}`
export const isDedicatedSlot = (slot: string | null | undefined): boolean =>
  !!slot && slot.startsWith(DEDICATED_PREFIX)

/// 产品固定的槽位名——ProviderManager 播种与绑定列表本地化名共用这份词表
///（负责人反馈 2026-09：绑定列表槽名全英文）。这些是 UI 词汇
///（providers.slot_<name>，对应 provider_config.rs 的固定槽：default/decision/jev
/// + ROLE_DRAFT_SLOT/BRIEF_SLOT/FLOW_DRAFT_SLOT）；角色自定义槽名（chat/code/…）
/// 是模板数据，原文照显不翻译。注意：只译绑定列表这一面——角色编辑器/侧栏的
/// 槽位下拉仍显示原槽名（选择器语境显示的是配置键，与后端 model_slot 值对齐）。
export const BUILTIN_SLOTS = ['default', 'decision', 'jev', 'role_draft', 'brief', 'flow_draft'] as const
export const isBuiltinSlot = (slot: string): boolean =>
  (BUILTIN_SLOTS as readonly string[]).includes(slot)

/// 跟随模式下可选的共享槽：排除专属槽（agent:* 是编辑器写出的实现细节，
/// 不该出现在「跟随谁」的选项里）。default 恒在（后端回退终点）。
export function sharedSlots(slots: ProvidersView['slots'], current?: string): string[] {
  const names = new Set(
    Object.keys(slots).filter((k) => !isDedicatedSlot(k)),
  )
  names.add('default')
  if (current && !isDedicatedSlot(current)) names.add(current)
  return [...names]
}

/// 槽位绑定的展示镜像：slot 未绑回落 default——与后端 resolve_slot 同链
/// （这里是只读展示，不回写；解析真相仍在后端）。
export function resolveBinding(
  slots: ProvidersView['slots'],
  slot: string | null | undefined,
): SlotBinding | undefined {
  return (slot ? slots[slot] : undefined) ?? slots['default']
}

/// 槽位 → 人读标签：专属槽显示绑定模型本身（agent:a3 是内部名，不裸示）；
/// 共享槽显示「槽名 · 模型」；未绑只给裸名。
export function slotLabel(
  slot: string | null | undefined,
  view: ProvidersView | null | undefined,
  dedicatedTag: string,
): string {
  if (!slot) return '—'
  const b = view ? resolveBinding(view.slots, slot) : undefined
  if (isDedicatedSlot(slot)) return b ? `${dedicatedTag} · ${b.model}` : dedicatedTag
  return b ? `${slot} · ${b.model}` : slot
}
