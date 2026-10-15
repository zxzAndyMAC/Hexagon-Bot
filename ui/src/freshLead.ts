// gap B3：首屏建议点名的领头角色（components C13）。
import type { PackDef } from './gen/PackDef'
import type { TeamRow } from './gen/TeamRow'

/** 首阶段的领头角色：第一阶段 roles 里第一个在团队中的；没有则 null（建议回落不点名）。 */
export function freshLead(pack: PackDef | null, team: TeamRow[]): { role: string; stage: string } | null {
  const first = pack?.stages[0]
  const role = first?.roles.find((r) => team.some((m) => m.role === r))
  return first && role ? { role, stage: first.name } : null
}
