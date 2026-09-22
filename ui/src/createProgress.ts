// 向导创建进度（票 14）。顺序与 setup.rs CREATE_STEP_ORDER 一致。
// 失败停在映射到的那一步：已报告且排在它前面的留下，它和后面的不算完成，
// 这样界面不会在密钥复查失败时仍显示「打开项目」已勾上。
import type { CreateStep as GeneratedCreateStep } from './gen/CreateStep'

export const CREATE_STEPS = [
  'check_dir',
  'git',
  'persist_roles',
  'recheck_keys',
  'open_project',
] as const

export type CreateStep = (typeof CREATE_STEPS)[number]

type Equal<A, B> = [A] extends [B] ? ([B] extends [A] ? true : never) : never
const _stepTypesMatch: Equal<CreateStep, GeneratedCreateStep> = true
void _stepTypesMatch

const CODE_STEP: Partial<Record<string, CreateStep>> = {
  dirty_tree: 'git',
  no_git: 'git',
  git_cli: 'git',
  not_repo: 'git',
  missing_keys: 'recheck_keys',
  unknown_role: 'persist_roles',
  stray_override: 'persist_roles',
  self_reviewer: 'persist_roles',
  unknown_override_reviewer: 'persist_roles',
  no_fast_role: 'persist_roles',
  agents_md_exists: 'check_dir',
}

export function applyCreateFailure(
  done: readonly CreateStep[],
  code: string,
): { done: CreateStep[]; failed: CreateStep } {
  const firstMissing = CREATE_STEPS.find((s) => !done.includes(s)) ?? 'open_project'
  const mapped = CODE_STEP[code]
  let failed = firstMissing
  if (mapped && CREATE_STEPS.indexOf(mapped) >= CREATE_STEPS.indexOf(firstMissing)) {
    failed = mapped
  }
  const cut = CREATE_STEPS.indexOf(failed)
  return {
    done: CREATE_STEPS.filter((s, i) => i < cut && done.includes(s)),
    failed,
  }
}
