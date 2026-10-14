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
