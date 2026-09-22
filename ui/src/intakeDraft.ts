import { api, errText } from './api'
import { useUiStore } from './store'

/// 负责人点头。不关输入框：草案条不是弹窗。
export async function confirmIntakeDraft() {
  try {
    await api.confirmIntakeBrief()
    useUiStore.setState({ intakeDraft: false })
    void useUiStore.getState().refreshFast().catch(() => {})
  } catch (e) {
    useUiStore.getState().pushToast(errText(e), 'err')
  }
}
