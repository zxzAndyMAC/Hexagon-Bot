// 切换项目（ui-redesign 票 13，interaction §13「切换项目阻塞」，feedback#errors）。
// 仍有未排空的电脑动作（desktop_status.busy）时不直接调 open_recent：
// 头部下方出 warn 横条「切换项目需要等待…」+「留在此项目」，排空后才真正切换。
// 为什么在 UI 侧等：宿主 replace_project 在旧回合停住前会持锁等待，期间界面无从解释为何不动；
// 未确认的原生排空会让宿主保留原项目（src-tauri replace_project 注释），不能报成已切换。
// 功能缺口 C1（负责人 2026-10-14 裁决）：等待前先调 desktop_leave_project——宿主真正暂停本项目持有的
// 电脑通道、取消进行中的动作并留 checkpoint，排空因此是「取消后停下」而不是等它自然做完。
// 「留在此项目」只撤回切换；已暂停的不自动恢复（恢复要负责人看过 checkpoint 后点「继续」，
// 被取消的动作结果未确认，不能悄悄接着跑）。宿主 close_project 也会补记 checkpoint，UI 这一步只是提前。
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

/** 要不要等排空。本项目持有电脑通道（忙或空闲占用）时先暂停 + 留 checkpoint；读不到状态按「没有动作」走。 */
async function pauseForLeave(): Promise<boolean> {
  const status = await api.desktopStatus().catch(() => null)
  if (!status) return false
  if (status.active_project && status.active_project === status.project_root) {
    const paused = await api.desktopLeaveProject(status.project_root)
    window.dispatchEvent(new Event('hexagon:desktop-status-changed'))
    return paused.busy
  }
  return status.busy
}

/** 「留在此项目」：撤回等待中的切换。 */
export function stayInProject() {
  useSwitchWait.setState({ wait: null })
}

/** 切到最近项目；有未排空的电脑动作时先等。返回是否真正发起了切换。 */
export async function switchProject(dir: string): Promise<boolean> {
  try {
    if (await pauseForLeave()) {
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
