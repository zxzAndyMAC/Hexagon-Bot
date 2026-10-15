// 建议弹层的数据形（ui-redesign 票 07，components D2）；渲染在 components/ComposerSuggestions.tsx。
export type MentionItem = { kind: '@'; label: string; agentId: string; name: string; inst: string | null; sleeping: boolean
  /** gap B7：灰职责（TeamRow.duty，职责首行；缺省不渲染） */
  duty?: string | null }
export type PathItem = { kind: '#'; label: string; path: string; dir: boolean; artifact: number | null }
export type SuggestItem = MentionItem | PathItem
export type SuggestPopup = { kind: '@' | '#'; items: SuggestItem[]; loading?: boolean; error?: boolean }

/** 弹层是否该画出来：有结果、首屏加载或错误时。空结果不出空壳。 */
export const popupVisible = (p: SuggestPopup | null): p is SuggestPopup =>
  !!p && (p.items.length > 0 || !!p.loading || !!p.error)

