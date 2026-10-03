import { bindingFor, matches, type ActionId } from './keymap'

// Standards review 2026-10-02: four native dialogs had separate lists and all
// missed confirmIntake. Native inert does not block window listeners. Keep this
// classification shared; emergency computer pause deliberately remains available.
const backgroundDecisions: ActionId[] = [
  'approve', 'reject', 'reconcileAction', 'abandonAction', 'retryAction',
  'acceptException', 'stageStamp', 'stageRewind', 'chooseDesignDirection', 'confirmIntake',
]
export function blockBackgroundDecision(event: KeyboardEvent): boolean {
  if (!backgroundDecisions.some(action => matches(event, bindingFor(action)))) return false
  event.preventDefault()
  event.stopImmediatePropagation()
  return true
}
