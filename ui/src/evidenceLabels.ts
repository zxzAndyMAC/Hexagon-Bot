import i18n from './i18n'
import type { ExceptionCandidate } from './gen/ExceptionCandidate'

export function exceptionLabel(item: ExceptionCandidate): string {
  const requirement = item.requirement
  return requirement.kind === 'check' && requirement.cmd.startsWith('quality:')
    ? i18n.t(`quality.category.${requirement.cmd.slice('quality:'.length)}`)
    : item.label
}
