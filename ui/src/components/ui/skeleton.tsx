// 骨架 Skeleton（components A20，feedback#loading）：圆角 6 条块，
// linear-gradient(90deg, --canvas, --hover-2, --canvas) 200% 宽、1.4s 扫过；减少动效时静止为 --canvas。
// 容器给 role=status + aria-busy（A20 聚焦行）；条块本身 aria-hidden。
import type { CSSProperties } from "react"
import { cn } from "@/lib/utils"

function Skeleton({ className, style }: { className?: string; style?: CSSProperties }) {
  return (
    <span
      data-slot="skeleton"
      aria-hidden
      className={cn(
        "block h-2.5 rounded-[var(--r-xs)] bg-[linear-gradient(90deg,var(--canvas),var(--hover-2),var(--canvas))] bg-[length:200%_100%] animate-hx-skl motion-reduce:animate-none motion-reduce:bg-canvas",
        className,
      )}
      style={style}
    />
  )
}

/** 时间线首屏 / 补载更早内容：30 圆角 9 头像块 + 三条（40% / 90% / 70%）。 */
function SkeletonRows({ count = 3, className }: { count?: number; className?: string }) {
  return (
    <>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} data-slot="skeleton-row" className={cn("flex gap-3 py-2.5", className)} aria-hidden>
          <Skeleton className="size-[30px] shrink-0 rounded-[9px]" />
          <div className="flex min-w-0 flex-1 flex-col gap-1.5 pt-0.5">
            <Skeleton style={{ width: "40%" }} /><Skeleton style={{ width: "90%" }} /><Skeleton style={{ width: "70%" }} />
          </div>
        </div>
      ))}
    </>
  )
}

export { Skeleton, SkeletonRows }
