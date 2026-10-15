// z 序上下文（foundations §4「模态内弹层」，2026-10-15 gap C5）：弹层 20 / tooltip 25 低于模态 30，
// 从模态里打开的菜单、Popover、Select、HoverCard、tooltip 若照默认 z 挂到 body，会被模态本身盖住
//（gap B3 在向导里实测过同类问题）。模态内容用 <InModalLayer> 包住，里面的浮层改用 35 / 36。
// 被否决的替代：把浮层 Portal 挂进模态容器——模态常带 overflow-hidden（命令面板），浮层会被裁掉。
import * as React from "react"

type Layer = "base" | "modal"
const LayerContext = React.createContext<Layer>("base")

export function InModalLayer({ children }: { children: React.ReactNode }) {
  return <LayerContext.Provider value="modal">{children}</LayerContext.Provider>
}

/** 浮层 z 类名：pop = 菜单 / Popover / Select / HoverCard；tip = tooltip。 */
export function useFloatingZ(kind: "pop" | "tip"): string {
  const inModal = React.useContext(LayerContext) === "modal"
  if (kind === "tip") return inModal ? "z-[36]" : "z-[25]"
  return inModal ? "z-[35]" : "z-20"
}
