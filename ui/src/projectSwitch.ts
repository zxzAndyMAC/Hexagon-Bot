// 切换项目（ui-redesign 票 13，interaction §13「切换项目阻塞」，feedback#errors）。
// 仍有未排空的电脑动作（desktop_status.busy）时不直接调 open_recent：
// 头部下方出 warn 横条「切换项目需要等待…」+「留在此项目」，排空后才真正切换。
// 为什么在 UI 侧等：宿主 replace_project 在旧回合停住前会持锁等待，期间界面无从解释为何不动；
// 未确认的原生排空会让宿主保留原项目（src-tauri replace_project 注释），不能报成已切换。
// 「留在此项目」只撤回这次切换请求，不取消、不暂停电脑动作（没有新 IPC，不改副作用边界）。
// 读不到 desktop_status（未开电脑操控 / 浏览器开发）按「没有动作」走——判定只影响要不要等，
// 不影响宿主自己的切换校验（fail-open 的代价 = 一次宿主侧等待，不产生未审副作用）。
import { create } from 'zustand'
import { api } from './api'
import { useUiStore } from './store'
import { toastError } from './toast'

export const SWITCH_POLL_MS = 1000

export interface SwitchWait {
  ticket: number
  dir: string
  /** 正在等待排空的当前项目名（横条文案）。 */
  project: string
}

export const useSwitchWait = create<{ wait: SwitchWait | null }>(() => ({ wait: null }))

let seq = 0
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const busy = () => api.desktopStatus().then((s) => s.busy, () => false)

/** 「留在此项目」：撤回等待中的切换。 */
export function stayInProject() {
  useSwitchWait.setState({ wait: null })
}

/** 切到最近项目；有未排空的电脑动作时先等。返回是否真正发起了切换。 */
export async function switchProject(dir: string): Promise<boolean> {
  try {
    if (await busy()) {
      const ticket = ++seq
      useSwitchWait.setState({ wait: { ticket, dir, project: useUiStore.getState().projectName } })
      for (;;) {
        await sleep(SWITCH_POLL_MS)
        if (useSwitchWait.getState().wait?.ticket !== ticket) return false // 留在此项目 / 被新请求取代
        if (!(await busy())) break
      }
      useSwitchWait.setState({ wait: null })
    }
    await api.openRecent(dir)
    await useUiStore.getState().refresh()
    return true
  } catch (e) {
    // TOCTOU：列出后目录被删仍会失败——toast 报出而不是静默拒绝
    useSwitchWait.setState({ wait: null })
    toastError(e)
    return false
  }
}
