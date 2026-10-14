// 日历 Calendar（components A23 右侧月历，usage#range）：shadcn Calendar（react-day-picker）按 Hexagon token 改写。
// 格 32×32 圆角 8；周标题 11.5 ink-3；区间内 --accent-tint 直角；起止 --accent 白字、外侧圆角 8；他月 ink-3 .5；
// 悬停 --hover 底；方向键移日、PgUp/PgDn 换月（react-day-picker 内建）；今天之后由调用方 disabled。
import * as React from "react"
import { DayPicker } from "react-day-picker"
import { ChevronLeft, ChevronRight } from "lucide-react"
import { cn } from "@/lib/utils"

const navButton =
  "hx-press hx-focus grid size-6 cursor-pointer place-items-center rounded-[var(--r-xs)] text-ink-2 live:hover:bg-hover live:hover:text-ink aria-disabled:cursor-default aria-disabled:opacity-45 [&_svg]:size-3"

function Calendar({ className, classNames, ...props }: React.ComponentProps<typeof DayPicker>) {
  return (
    <DayPicker
      showOutsideDays
      className={cn("w-fit text-ink", className)}
      classNames={{
        months: "relative flex",
        month: "flex flex-col",
        nav: "absolute inset-x-0 top-0 flex items-center justify-between",
        button_previous: navButton,
        button_next: navButton,
        month_caption: "mb-2 flex h-6 items-center justify-center text-[13px] font-semibold",
        caption_label: "select-none",
        month_grid: "border-separate border-spacing-x-0 border-spacing-y-0.5",
        weekdays: "",
        weekday: "size-8 p-0 text-center text-[11.5px] font-normal text-ink-3 select-none",
        week: "",
        day: "group/day size-8 p-0 text-center text-[12.5px] tabular-nums",
        day_button:
          "hx-focus size-8 cursor-pointer rounded-[8px] live:hover:bg-hover group-data-[selected=true]/day:hover:bg-transparent disabled:cursor-default",
        range_start: "rounded-l-[8px] bg-primary text-white",
        range_end: "rounded-r-[8px] bg-primary text-white",
        range_middle: "rounded-none bg-accent-tint",
        selected: "",
        today: "font-semibold",
        outside: "text-ink-3 opacity-50",
        disabled: "text-ink-3 opacity-45",
        hidden: "invisible",
        ...classNames,
      }}
      components={{
        Chevron: ({ orientation }) => (orientation === "left" ? <ChevronLeft aria-hidden /> : <ChevronRight aria-hidden />),
      }}
      {...props}
    />
  )
}

export { Calendar }
