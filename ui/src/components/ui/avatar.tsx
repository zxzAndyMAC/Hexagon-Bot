// 头像 Avatar（components A13）：方圆角色块，确定色底（foundations §1.5 c1–c8，按 agent id 哈希）白字首字；
// 尺寸 18/24/30/40（圆角 5/7/9/11）；右下 9px 状态点（绿在线 / --ink-3 休眠），外描 2px --app-side。
// 负责人头像圆形 c8。自定义图加载中先显示首字色块，读取失败回退首字（Radix Avatar 的 Fallback）。
// 可换图（传 onChangeImage）时悬停出 --scrim 覆层 + 相机图标。叠放用 AvatarStack。
import * as React from "react"
import { Avatar as AvatarPrimitive } from "radix-ui"
import { cva } from "class-variance-authority"
import { Camera } from "lucide-react"
import { cn } from "@/lib/utils"
import { agentSwatch } from "@/colors"

const SIZES = {
  18: "size-[18px] rounded-[5px] text-[10px]",
  // 补设计（票 06）：回到底部的叠放头像 20（C14 / workbench#jump `.jump .stack .av` 圆角 6、字 10）
  20: "size-5 rounded-[var(--r-xs)] text-[10px]",
  24: "size-6 rounded-[7px] text-[11.5px]",
  30: "size-[30px] rounded-[9px] text-[13px]",
  40: "size-10 rounded-[11px] text-[16px]",
} as const

// c1–c8 写成字面量（Tailwind 只扫源码文本）
const SWATCH = [
  "bg-[var(--agent-c1)]", "bg-[var(--agent-c2)]", "bg-[var(--agent-c3)]", "bg-[var(--agent-c4)]",
  "bg-[var(--agent-c5)]", "bg-[var(--agent-c6)]", "bg-[var(--agent-c7)]", "bg-[var(--agent-c8)]",
]

const statusVariants = cva(
  "absolute -right-0.5 -bottom-0.5 size-[9px] rounded-full shadow-[0_0_0_2px_var(--app-side)]",
  { variants: { status: { online: "bg-green", sleep: "bg-ink-3" } } }
)

type AvatarProps = {
  /** 确定色的哈希源；负责人不传，固定 c8 圆形。 */
  agentId?: string
  name: string
  size?: keyof typeof SIZES
  src?: string
  status?: "online" | "sleep"
  /** 休眠：只把头像本体半透明去饱和（状态点与相邻文字不变）。owner-followups-1015 票 09：
   *  原先整行 opacity .5，名字和「休眠」字一起掉到 AA 以下；规范 foundations「对比度」改为只淡头像。 */
  dimmed?: boolean
  owner?: boolean
  onChangeImage?: () => void
  changeImageLabel?: string
  onImageError?: () => void
  className?: string
}

function Avatar({
  agentId,
  name,
  size = 24,
  src,
  status,
  dimmed = false,
  owner = false,
  onChangeImage,
  changeImageLabel,
  onImageError,
  className,
}: AvatarProps) {
  const swatch = owner ? 8 : agentSwatch(agentId ?? name)
  const shape = cn(SIZES[size], owner && "rounded-full")
  const face = (
    <AvatarPrimitive.Root
      data-slot="avatar"
      data-swatch={swatch}
      data-dimmed={dimmed || undefined}
      className={cn("relative inline-grid shrink-0 place-items-center font-semibold text-white select-none", shape, SWATCH[swatch - 1], dimmed && "opacity-50 grayscale")}
    >
      {src && (
        <AvatarPrimitive.Image
          src={src}
          alt={name}
          className={cn("size-full object-cover", shape)}
          onLoadingStatusChange={(s) => { if (s === "error") onImageError?.() }}
        />
      )}
      <AvatarPrimitive.Fallback aria-label={src ? undefined : name}>{name.slice(0, 1) || "?"}</AvatarPrimitive.Fallback>
    </AvatarPrimitive.Root>
  )
  const body = (
    <>
      {face}
      {status && <span data-slot="avatar-status" data-status={status} className={statusVariants({ status })} />}
    </>
  )
  if (onChangeImage) {
    return (
      <button
        type="button"
        aria-label={changeImageLabel ?? name}
        onClick={onChangeImage}
        className={cn("group/avatar hx-press hx-focus relative inline-grid cursor-pointer", shape, className)}
      >
        {body}
        <span className={cn("absolute inset-0 grid place-items-center bg-scrim text-white opacity-0 transition-opacity duration-150 group-hover/avatar:opacity-100", shape)}>
          <Camera className="size-1/2" strokeWidth={1.75} aria-hidden />
        </span>
      </button>
    )
  }
  return <span className={cn("relative inline-grid shrink-0", className)}>{body}</span>
}

/** 叠放：后一个左移 -6、外描 2px --surface（workbench#jump）。 */
function AvatarStack({ className, children }: { className?: string; children: React.ReactNode }) {
  return (
    <span data-slot="avatar-stack" className={cn("inline-flex items-center *:not-first:-ml-1.5 [&_[data-slot=avatar]]:shadow-[0_0_0_2px_var(--surface)]", className)}>
      {children}
    </span>
  )
}

export { Avatar, AvatarStack }
