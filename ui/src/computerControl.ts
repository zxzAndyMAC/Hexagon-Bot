// 电脑操控的纯判定（components D19，票 ui-redesign 14）：头部控制状态显示什么、暂停按钮何时出现、
// 浏览器「打开 / 连接」为什么禁用。只读宿主状态，不改授权或暂停语义（Owner 2026-10-06：
// 同意与负责人暂停都不得被隐式改变）。
import type { DesktopStatus } from './gen/DesktopStatus'

export type ControlTarget = 'browser' | 'computer'
export type ControlSummary =
  | { kind: 'hidden' }
  | { kind: 'active'; agent: string; target: ControlTarget }
  | { kind: 'elsewhere' }
  | { kind: 'paused' }
  | { kind: 'checkpoint' }
  | { kind: 'idle'; target: ControlTarget }

/** 头部 chip：谁在控制什么。电脑操作停用且没有浏览器连接时不占头部位置（「可插入控制状态」B5）。 */
export function controlSummary(status: DesktopStatus | null, browserConnected: boolean, nameOf: (agentId: string) => string): ControlSummary {
  if (!status) return { kind: 'hidden' }
  // 功能缺口 C1：离开项目时留下的 checkpoint 优先——回来第一眼看到「已暂停 · 可继续」。
  if (status.checkpoint) return { kind: 'checkpoint' }
  if (status.paused) return { kind: 'paused' }
  if (status.active_agent) {
    // Issue19：别的项目持有控制时不把本项目角色名安到它头上。
    if (status.active_project !== null && status.active_project !== status.project_root) return { kind: 'elsewhere' }
    return { kind: 'active', agent: nameOf(status.active_agent), target: browserConnected ? 'browser' : 'computer' }
  }
  if (status.enabled || browserConnected) return { kind: 'idle', target: browserConnected ? 'browser' : 'computer' }
  return { kind: 'hidden' }
}

/** 「暂停电脑操作」只在还有东西可能动手时出现；已暂停时由面板里的「恢复」接手。 */
export function canPauseComputer(status: DesktopStatus | null): boolean {
  return !!status && !status.paused && (status.enabled || !!status.active_agent)
}

export type LaunchBlock = 'loading' | 'disabled' | 'paused' | null
/** 浏览器「打开」「连接」的禁用原因；安装引导不受此限（D19 禁用行）。 */
export function browserLaunchBlock(status: DesktopStatus | null): LaunchBlock {
  if (!status) return 'loading'
  if (!status.enabled) return 'disabled'
  if (status.paused) return 'paused'
  return null
}
