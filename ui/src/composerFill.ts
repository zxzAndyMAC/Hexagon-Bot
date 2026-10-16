import { useEffect, useSyncExternalStore } from 'react'

// 「只填入输入条、不发送」的唯一入口（ui-redesign 票 05：归来摘要建议下一步、
// 首屏建议卡；components C12 / C13）。输入条是唯一的发送面——这里只发一个窗口事件，
// 由 Composer 把文字放进草稿并聚焦；绝不调 api.sendMessage。
// 被否决：把草稿提到 store。输入条草稿、附件、引用都是 Composer 的局部态，
// 提 store 会牵动 ADR 0090 的「只隐藏不卸载」保留语义，本票不碰。
export const COMPOSER_FILL_EVENT = 'hexagon:composer-fill'

export function fillComposer(text: string) {
  window.dispatchEvent(new CustomEvent<string>(COMPOSER_FILL_EVENT, { detail: text }))
}

/** 已有草稿时追加到新一行，不覆盖负责人写了一半的话。 */
export function mergeFill(draft: string, fill: string): string {
  return draft.trim() ? `${draft.replace(/\s+$/, '')}\n${fill}` : fill
}

// 首屏期间输入条换占位（workbench#fresh「描述你要做的软件，或者点上面的建议」；components C13 补设计，票 16a）。
// 与填入同理不进 store：首屏是否显示由 Timeline 判定（需要窗口分页的 has_before），
// FreshStart 挂载即计数 +1、卸载 -1，Composer 只订阅「>0」。
let freshMounted = 0
const freshSubs = new Set<() => void>()
const freshNotify = () => { for (const f of freshSubs) f() }
const freshSubscribe = (f: () => void) => { freshSubs.add(f); return () => { freshSubs.delete(f) } }

/** FreshStart 调：挂载期间输入条显示首屏占位。 */
export function useFreshStartMarker() {
  useEffect(() => {
    freshMounted += 1
    freshNotify()
    return () => { freshMounted -= 1; freshNotify() }
  }, [])
}

/** Composer 调：首屏引导是否正在显示。 */
export function useFreshStartShown(): boolean {
  return useSyncExternalStore(freshSubscribe, () => freshMounted > 0)
}
