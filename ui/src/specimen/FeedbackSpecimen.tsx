// 反馈层标本（ui-redesign 票 13）：按原型 feedback.html 的节次（toast / empty / loading / errors）摆共享件，
// 用来和 `feedback.html?theme=…&focus=<节>` 同尺寸并排截图。开发工具页：文案不进 i18n。
import type { ReactNode } from 'react'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Empty } from '@/components/ui/empty'
import { InlineError } from '@/components/ui/inline-error'
import { SkeletonRows } from '@/components/ui/skeleton'
import { PixelGrid, Shimmer } from '@/components/ui/spinner'
import { ToastPill } from '@/components/ui/sonner'

function Sec({ id, n, title, desc, children }: { id: string; n: string; title: string; desc: string; children: ReactNode }) {
  return (
    <section id={`fb-${id}`} className="px-8 pt-8">
      <h2 className="mb-3 text-[14px] font-semibold"><span className="mr-2 font-mono text-[11px] text-ink-3">{n}</span>{title} <span className="font-normal text-ink-3">{desc}</span></h2>
      <div className="rounded-[var(--r-panel)] bg-canvas p-6">{children}</div>
    </section>
  )
}

export function FeedbackSpecimen() {
  return (
    <>
      <Sec id="toast" n="01" title="Toast" desc="feedback#toast · A18">
        <div className="flex flex-col items-center gap-3">
          <ToastPill tone="ok" text="已导出 412 条轨迹" action="打开文件夹" />
          <ToastPill tone="info" icon="undo" text="已撤销规则「npm test *」" action="撤回" />
          <ToastPill tone="err" text="切换项目失败：目录不存在" action="从最近项目移除" />
        </div>
      </Sec>
      <Sec id="empty" n="03" title="空态" desc="feedback#empty · A19">
        <div className="grid grid-cols-3 gap-4">
          <Card><Empty icon="layers" title="还没有产物" desc="角色交付规格、界面稿或代码后会出现在这里。" action={<Button size="sm">查看流程</Button>} /></Card>
          <Card><Empty icon="inbox" title="没有待决" desc="需要你拍板的事会停靠在输入条上方。" action={<Button size="sm"><Icon name="keyboard" size={12} />练习裁决快捷键</Button>} /></Card>
          <Card><Empty icon="search" title="没找到「部署」" desc="试试「发布」或「检验」；危险指令也在这里。" action={<Button size="sm">清空搜索</Button>} /></Card>
          <Card><Empty icon="chart-column" title="这段时间没有用量" desc="换一个时间范围，或者等团队开始工作。" action={<Button size="sm">近 30 天</Button>} /></Card>
          <Card><Empty icon="lock" title="需要先打开项目" desc="权限和用量属于项目状态，无项目时不可用。" action={<Button size="sm" variant="primary">打开项目</Button>} /></Card>
          <Card><Empty icon="plug" title="还没有 MCP 服务" desc="从本机已有的客户端导入，或者手动添加。" action={<><Button size="sm">扫描本机</Button><Button size="sm">新建</Button></>} /></Card>
        </div>
      </Sec>
      <Sec id="loading" n="04" title="加载" desc="feedback#loading · A20">
        <div className="grid grid-cols-3 gap-4">
          <Card className="p-4"><div className="mb-3 text-[12px] text-ink-3">首个 token 等待</div>
            <div className="flex items-center gap-2 text-[13px]"><PixelGrid /><Shimmer>正在思考</Shimmer><span className="font-mono text-ink-3">4.2s</span>
              {/* 等网变体（C9 `.tl-wait.net`：标签换橙色「重连中」） */}
              <PixelGrid /><span className="text-orange">重连中</span><span className="font-mono text-ink-3">0:18</span></div></Card>
          <Card className="p-4"><div className="mb-3 text-[12px] text-ink-3">时间线首屏 / 补载更早内容</div>
            <div role="status" aria-busy="true"><SkeletonRows count={1} className="py-0" /></div></Card>
          <Card className="p-4"><div className="mb-3 text-[12px] text-ink-3">按钮内的异步操作</div>
            <div className="flex gap-1.5"><Button size="sm" loading>检测中…</Button><Button size="sm" variant="primary" loading>保存中</Button></div></Card>
        </div>
      </Sec>
      <Sec id="errors" n="05" title="故障与恢复" desc="feedback#errors · C5">
        <div className="mx-auto flex max-w-[720px] flex-col gap-4">
          <InlineError title="模型请求失败：钥匙无效" code="401" meta="deepseek-v4"
            desc="这次请求没有执行任何工具。更新钥匙后只重试这一次请求，不会重放之前的动作。"
            actions={<><Button size="sm"><Icon name="key" size={12} />更新钥匙</Button><Button size="sm" variant="ghost">换一个模型</Button></>} />
          <InlineError variant="row" title="更早的内容加载失败" onRetry={() => {}} retryLabel="重试" />
          <div className="flex items-center gap-2 rounded-[12px] bg-orange-tint px-3 py-2.5 text-[13px] text-ink">
            <Icon name="warn" size={16} className="shrink-0 text-orange" />
            <span className="min-w-0 flex-1"><b className="font-semibold">切换项目需要等待</b>：todo-pwa 还有电脑动作没有排空，排空后才会切换。</span>
            <Button size="xs">留在此项目</Button>
          </div>
        </div>
      </Sec>
    </>
  )
}
