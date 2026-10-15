// 电脑操控状态共享读（功能缺口 C4，票 14 留尾；同票 08 useDelegation 的做法）。
// 之前头部 chip、电脑操作面板、项目浏览器面板、设置的暂停按钮各开一个 2 秒定时器（任务前检查卡 5 秒），
// 弹层打开时同一时刻 3–4 路 desktop_status + browser_status 并发。现在只有一个定时器：
// 有订阅者才轮询（隐藏窗口跳过），focus 与 `hexagon:desktop-status-changed` 触发重读，最后一个订阅者卸下即停并清空。
// 读结果按项目 epoch 标记：切项目后旧读立即作废（Issue14 P2），晚到的旧 epoch 读丢弃。
// 只读：不改同意 / 暂停语义（Owner 2026-10-06）。Agent 屏幕（1 秒取帧）与元素引用（1.2 秒取选择）
// 读的是各自的预览 / 选择通道，节奏不同，不并入。
import { useEffect } from 'react'
import { create } from 'zustand'
import { api, errText } from './api'
import { useUiStore } from './store'
import type { BrowserSession } from './gen/BrowserSession'
import type { DesktopStatus } from './gen/DesktopStatus'

export const DESKTOP_POLL_MS = 2000
export const DESKTOP_STATUS_CHANGED = 'hexagon:desktop-status-changed'

type DesktopRead = {
  epoch: number
  /** 每次写入递增：订阅方据此对齐「又读到一次」（如面板复查系统授权）。 */
  stamp: number
  status: DesktopStatus | null
  browser: BrowserSession | null
  /** desktop_status 读失败原因。 */
  error: string
  /** 浏览器会话读失败原因；不作废已读到的电脑状态（Owner 2026-10-06）。 */
  browserError: string
}

const EMPTY: DesktopRead = { epoch: -1, stamp: 0, status: null, browser: null, error: '', browserError: '' }
const useDesktopRead = create<DesktopRead>(() => EMPTY)

let subscribers = 0
let generation = 0
let timer = 0
let queued = false
let justPublished = false

const nextStamp = () => useDesktopRead.getState().stamp + 1

/** 立即重读。最新请求为准：更早发出、更晚返回的读一律丢弃（票 14 晚到轮询不得撤销已完成的暂停）。
 * 切项目进行中（epoch>0 且根未知）不读——宿主此刻可能仍是旧项目，读回来会被记到新 epoch 上。 */
export async function refreshDesktopStatus(): Promise<void> {
  if (subscribers === 0) return
  const { projectEpoch: epoch, projectRoot } = useUiStore.getState()
  if (epoch > 0 && projectRoot === null) return
  const ticket = ++generation
  const fresh = () => ticket === generation && subscribers > 0 && useUiStore.getState().projectEpoch === epoch
  try {
    const status = await api.desktopStatus()
    if (!fresh()) return
    let browser: BrowserSession | null = null
    let browserError = ''
    try { browser = await api.browserStatus(status.project_root) } catch (e) { browserError = errText(e) }
    if (!fresh()) return
    useDesktopRead.setState({ epoch, stamp: nextStamp(), status, browser, error: '', browserError })
  } catch (e) {
    if (fresh()) useDesktopRead.setState({ epoch, stamp: nextStamp(), status: null, browser: null, error: errText(e), browserError: '' })
  }
}

/** 同一 tick 的多个挂载点 / 切换信号合并成一次读。 */
function queueRefresh() {
  if (queued) return
  queued = true
  queueMicrotask(() => { queued = false; void refreshDesktopStatus() })
}

/** 写操作（desktop_control 等）返回的宿主状态直接发布，作废在途的旧读；紧随其后的
 * `hexagon:desktop-status-changed`（同步派发）不再触发重读——发布的就是宿主刚返回的状态
 * （同换皮前面板写入期间跳过轮询的语义）。浏览器会话沿用上次读。 */
export function publishDesktopStatus(status: DesktopStatus) {
  generation++
  justPublished = true
  void Promise.resolve().then(() => { justPublished = false })
  const epoch = useUiStore.getState().projectEpoch
  const previous = useDesktopRead.getState()
  useDesktopRead.setState({
    epoch, stamp: nextStamp(), status, error: '',
    browser: previous.epoch === epoch && previous.browser?.project_root === status.project_root ? previous.browser : null,
    browserError: previous.epoch === epoch ? previous.browserError : '',
  })
}

const onSignal = () => { if (!justPublished) void refreshDesktopStatus() }

function start() {
  timer = window.setInterval(() => { if (!document.hidden) void refreshDesktopStatus() }, DESKTOP_POLL_MS)
  window.addEventListener('focus', onSignal)
  window.addEventListener(DESKTOP_STATUS_CHANGED, onSignal)
}

function stop() {
  window.clearInterval(timer)
  window.removeEventListener('focus', onSignal)
  window.removeEventListener(DESKTOP_STATUS_CHANGED, onSignal)
  generation++; queued = false
  useDesktopRead.setState(EMPTY, true)
}

export type DesktopStatusView = Omit<DesktopRead, 'epoch'> & { refresh: () => Promise<void> }

/** 订阅共享读；只返回当前项目 epoch 的结果。 */
export function useDesktopStatus(): DesktopStatusView {
  const epoch = useUiStore((s) => s.projectEpoch)
  const root = useUiStore((s) => s.projectRoot)
  const read = useDesktopRead()
  useEffect(() => {
    subscribers += 1
    if (subscribers === 1) start()
    return () => { subscribers -= 1; if (subscribers === 0) stop() }
  }, [])
  // 挂载、切项目、项目根落定都补读一次；同 tick 多个挂载点合并为一次读。
  useEffect(() => { queueRefresh() }, [epoch, root])
  const current = read.epoch === epoch
  return {
    stamp: current ? read.stamp : 0,
    status: current ? read.status : null,
    browser: current ? read.browser : null,
    error: current ? read.error : '',
    browserError: current ? read.browserError : '',
    refresh: refreshDesktopStatus,
  }
}
