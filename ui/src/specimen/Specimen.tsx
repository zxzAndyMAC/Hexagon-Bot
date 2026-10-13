// 组件标本页（ui-redesign 票 01，仅开发环境：`npm run dev` 后打开 http://localhost:1420/?specimen）。
// 每个基础组件（components A1–A17）一行，逐格展示状态矩阵的 7 个状态；悬停/按下/聚焦用 data-force 钉成静态
// （index.css 的 hover / focus-within 变体与 hx-press / hx-focus 认它）。顶部切主题（浅 / 深 / 跟随系统），
// 用来和原型 index#tokens、index#type 的 `?theme=light|dark` 截图并排比对。
// 开发工具页：文案不进 i18n（不随产品发布，生产构建里整页被摇掉）。
import { useEffect, useState, type ReactNode } from 'react'
import { Icon } from '@/components/Icon'
import { Button, SplitButton } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { Kbd, KbdGroup, ShortcutKbd } from '@/components/ui/kbd'
import { Badge } from '@/components/ui/badge'
import { CodeChip } from '@/components/ui/code-chip'
import { Dot } from '@/components/ui/dot'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { Card, CardButton, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Input, Textarea } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Checkbox } from '@/components/ui/checkbox'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Progress } from '@/components/ui/progress'
import { Avatar, AvatarStack } from '@/components/ui/avatar'
import { PixelGrid, Shimmer, Spinner } from '@/components/ui/spinner'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuError, DropdownMenuFooter, DropdownMenuSkeleton, DropdownMenuGroup, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Dialog, DialogContent, DialogDescription, DialogError, DialogFooter, DialogHeader, DialogIcon, DialogTitle,
} from '@/components/ui/dialog'
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogIcon, AlertDialogTitle, AlertDialogTrigger,
} from '@/components/ui/alert-dialog'

const STATES = ['默认', '悬停', '按下', '聚焦', '禁用', '加载', '错误'] as const
type State = (typeof STATES)[number]
const FORCE: Partial<Record<State, string>> = { 悬停: 'hover', 按下: 'hover press', 聚焦: 'focus' }

type Row = {
  id: string
  name: string
  /** 返回该状态的标本；返回字符串 = 不适用及原因。 */
  render: (s: State) => ReactNode | string
}

function Cell({ state, children }: { state: State; children: ReactNode | string }) {
  if (typeof children === 'string') {
    return <div className="flex min-h-14 items-center text-[11.5px] text-ink-3">{children}</div>
  }
  return (
    <div data-force={FORCE[state]} className="flex min-h-14 flex-wrap items-center gap-2">
      {children}
    </div>
  )
}

/** 非模态、挂在格子里的静态模态（transform 让 fixed 定位相对格子），只为截图比对。 */
function StaticModal({ state }: { state: State }) {
  const [host, setHost] = useState<HTMLDivElement | null>(null)
  const busy = state === '加载'
  return (
    <div ref={setHost} className="relative h-[190px] w-[260px] overflow-hidden rounded-[12px] [transform:translateZ(0)]">
      {host && (
        <Dialog open modal={false}>
          <DialogContent
            container={host}
            busy={busy}
            data-force={FORCE[state]}
            onOpenAutoFocus={(e) => e.preventDefault()}
            className="w-[244px] p-4"
          >
            <DialogHeader>
              <DialogIcon tone="err"><Icon name="publish" /></DialogIcon>
              <DialogTitle>发布到生产？</DialogTitle>
            </DialogHeader>
            <DialogDescription>发布不可撤回，外部用户会立即看到。</DialogDescription>
            {state === '错误' && <DialogError>发布失败：网络不可达，重试或稍后再来。</DialogError>}
            <DialogFooter>
              <Button size="sm" disabled={busy}>取消 <Kbd>Esc</Kbd></Button>
              <Button size="sm" variant="danger" loading={busy}>{busy ? '发布中' : '发布'}</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </div>
  )
}

const ROWS: Row[] = [
  {
    id: 'A1', name: '按钮 Button',
    render: (s) => s === '错误' ? '不适用（失败走 toast 或旁侧 help 红字）' : (
      <>
        {(['default', 'primary', 'ink', 'danger', 'ghost', 'quiet'] as const).map((v) => (
          <Button key={v} variant={v} size="sm" disabled={s === '禁用'} loading={s === '加载'}>
            {s === '加载' ? '保存中' : v}
            {v === 'primary' && s === '默认' && <Kbd>⌘↵</Kbd>}
          </Button>
        ))}
        {s === '默认' && <SplitButton size="sm" menuLabel="更多批准方式">批准</SplitButton>}
        {s === '默认' && (['xs', 'md', 'lg'] as const).map((z) => <Button key={z} size={z}>{z}</Button>)}
      </>
    ),
  },
  {
    id: 'A2', name: '图标按钮 IconButton',
    render: (s) => s === '错误' ? '不适用（错误由所属区域显示）' : (
      <>
        <IconButton label="开关画布" action="toggleCanvas" disabled={s === '禁用'} disabledReason="没有打开的项目" loading={s === '加载'}>
          <Icon name="panel-right" size={16} />
        </IconButton>
        <IconButton label="开关画布" on disabled={s === '禁用'} loading={s === '加载'}><Icon name="panel-right" size={16} /></IconButton>
        <IconButton label="复制" size="sm" disabled={s === '禁用'} loading={s === '加载'}><Icon name="copy" /></IconButton>
      </>
    ),
  },
  {
    id: 'A3', name: '键位 Kbd',
    render: (s) => s === '默认' ? (
      <>
        <Kbd>⌘K</Kbd>
        <KbdGroup><Kbd>⌘</Kbd><Kbd>↵</Kbd></KbdGroup>
        <ShortcutKbd binding="mod+shift+Enter" />
        <Button size="sm" variant="primary">批准 <Kbd>⌘↵</Kbd></Button>
      </>
    ) : s === '禁用' ? <Button size="sm" disabled>批准 <Kbd>⌘↵</Kbd></Button>
      : s === '错误' ? '不适用（键位冲突提示在设置键盘行 E7）' : '不适用（非交互，印刷字）',
  },
  {
    id: 'A4', name: '胶囊 Chip',
    render: (s) => s === '默认' ? (
      <>
        <Badge>中性</Badge><Badge variant="ok"><Dot tone="on" />通过</Badge><Badge variant="warn">待复审</Badge>
        <Badge variant="err">失败</Badge><Badge variant="info">托管</Badge><Badge variant="flag">升级</Badge>
        <Badge variant="line">line</Badge><Badge variant="ok" shape="sq">sq</Badge>
      </>
    ) : s === '加载' ? <Badge loading>执行中</Badge>
      : s === '错误' ? <Badge variant="err"><Icon name="warn" size={12} />测试失败</Badge>
      : '不适用（非交互，只表状态）',
  },
  {
    id: 'A5', name: '代码 chip CodeChip',
    render: (s) => s === '默认' ? (
      <><CodeChip>cargo test -p core</CodeChip><CodeChip file added={12} removed={3}>src/api.rs</CodeChip></>
    ) : (s === '悬停' || s === '按下' || s === '聚焦') ? <CodeChip file added={12} removed={3} onClick={() => {}}>src/api.rs</CodeChip>
      : s === '错误' ? <CodeChip file missing missingLabel="文件已移除">src/old.rs</CodeChip>
      : '不适用（不可点即纯展示）',
  },
  {
    id: 'A6', name: '状态点 Dot',
    render: (s) => s === '默认' ? (
      <><Dot /><Dot tone="on" /><Dot tone="warn" /><Dot tone="err" /><Dot tone="info" /></>
    ) : s === '禁用' ? <Dot tone="off" /> : s === '加载' ? <Dot tone="info" pulse /> : s === '错误' ? <Dot tone="err" />
      : '不适用（非交互）',
  },
  {
    id: 'A7', name: '分段控件 Segmented',
    render: (s) => s === '错误' ? '不适用（错误显示在内容区）' : (
      <Segmented value="all" onValueChange={() => {}} aria-label="过滤">
        <SegmentedItem value="all">全部</SegmentedItem>
        <SegmentedItem value="msg">消息</SegmentedItem>
        <SegmentedItem value="diff" disabled={s === '禁用'} disabledReason="只有一个版本，无法对比">对比</SegmentedItem>
      </Segmented>
    ),
  },
  {
    id: 'A8', name: '表面 Card',
    render: (s) => s === '默认' ? (
      <>
        {(['card', 'raised', 'overlay', 'inset'] as const).map((v) => (
          <Card key={v} surface={v} className="h-12 w-20 items-center justify-center text-[12px] text-ink-2">{v}</Card>
        ))}
      </>
    ) : (s === '悬停' || s === '按下' || s === '聚焦') ? (
      <CardButton className="w-44 px-3 py-2.5 text-[13px]">交付卡（可点）</CardButton>
    ) : s === '加载' ? (
      <Card className="w-44 gap-1.5 p-3"><Progress loading aria-label="加载中" /><Progress loading aria-label="加载中" /></Card>
    ) : s === '错误' ? (
      <Card className="w-56"><CardHeader><CardTitle>用量</CardTitle></CardHeader><CardContent className="text-red">读取失败 · 重试</CardContent></Card>
    ) : '不适用（禁用落在卡内按钮）',
  },
  {
    id: 'A9', name: '输入框 Field',
    render: (s) => s === '按下' ? '不适用（文本域无按下）' : (
      <div className="flex w-64 flex-col gap-2">
        <Field label="API Key" error={s === '错误' ? '密钥格式不对：应以 sk- 开头，检查后重填。' : undefined} help="只存本机钥匙串">
          <Input
            placeholder="sk-…"
            disabled={s === '禁用'}
            aria-invalid={s === '错误' || undefined}
            loadingLabel={s === '加载' ? '检测中…' : undefined}
            trailing={s === '默认' ? <span className="text-[12px] whitespace-nowrap text-green">检测通过</span> : undefined}
          />
        </Field>
        {s === '默认' && <Textarea placeholder="补充说明" rows={2} />}
      </div>
    ),
  },
  {
    id: 'A10', name: '开关 Switch',
    render: (s) => (
      <div className="flex flex-col gap-1">
        <div className="flex items-center gap-3">
          <Switch aria-label="关" checked={false} disabled={s === '禁用'} loading={s === '加载'} />
          <Switch aria-label="开" checked disabled={s === '禁用'} />
        </div>
        {s === '错误' && <span className="text-[12px] text-red">托管未能开启</span>}
      </div>
    ),
  },
  {
    id: 'A11', name: '复选 / 单选',
    render: (s) => s === '加载' ? '不适用（本地即时状态）' : (
      <div className="flex flex-col gap-1">
        <div className="flex items-center gap-3">
          <Checkbox aria-label="未选" checked={false} disabled={s === '禁用'} aria-invalid={s === '错误' || undefined} />
          <Checkbox aria-label="已选" checked disabled={s === '禁用'} />
          <RadioGroup value="b" className="flex gap-3" disabled={s === '禁用'} aria-label="模式">
            <RadioGroupItem value="a" aria-label="a" aria-invalid={s === '错误' || undefined} />
            <RadioGroupItem value="b" aria-label="b" />
          </RadioGroup>
        </div>
        {s === '错误' && <span className="text-[12px] text-red">请先勾选「我了解放弃追查后不会重放」</span>}
      </div>
    ),
  },
  {
    id: 'A12', name: '进度条 Progress',
    render: (s) => s === '默认' ? (
      <div className="flex w-48 flex-col gap-2">
        <Progress value={42} aria-label="ink" /><Progress value={64} tone="accent" aria-label="accent" /><Progress value={86} tone="warn" aria-label="warn" />
      </div>
    ) : s === '加载' ? <div className="w-48"><Progress loading aria-label="加载中" /></div>
      : s === '错误' ? <div className="w-48"><Progress value={100} tone="cap" aria-label="触顶" /></div>
      : s === '禁用' ? <span className="text-[12px] text-ink-2 tabular-nums">¥12.40（无上限，不渲染条）</span>
      : '不适用（非交互）',
  },
  {
    id: 'A13', name: '头像 Avatar',
    render: (s) => s === '默认' ? (
      <>
        {([18, 24, 30, 40] as const).map((z, i) => <Avatar key={z} agentId={`a${i}`} name="设计" size={z} status={z === 24 ? 'online' : undefined} />)}
        <Avatar owner name="我" size={24} />
        <AvatarStack><Avatar agentId="a1" name="产" /><Avatar agentId="a2" name="开" /><Avatar agentId="a3" name="测" /></AvatarStack>
      </>
    ) : (s === '悬停' || s === '按下' || s === '聚焦') ? <Avatar agentId="a4" name="研" size={40} onChangeImage={() => {}} changeImageLabel="更换头像" />
      : s === '禁用' ? <span className="opacity-50"><Avatar agentId="a5" name="休" status="sleep" /></span>
      : s === '加载' ? <Avatar agentId="a6" name="载" size={30} src="data:image/png;base64,invalid" />
      : <Avatar agentId="a7" name="错" size={30} src="/missing-avatar.png" />,
  },
  {
    id: 'A14', name: '加载原语 Loader',
    render: (s) => s === '默认' || s === '加载' ? (
      <>
        <span className="inline-flex items-center gap-2 text-[13px]"><PixelGrid /><Shimmer>正在思考</Shimmer><span className="font-mono text-[12px] text-ink-3 tabular-nums">4.2s</span></span>
        <Spinner label="加载中" />
      </>
    ) : s === '错误' ? (
      <span className="inline-flex items-center gap-2 text-[13px]"><PixelGrid /><Shimmer>重连中</Shimmer><span className="font-mono text-[12px] text-ink-3 tabular-nums">12s</span></span>
    ) : '不适用（role=status，不可交互）',
  },
  {
    id: 'A15', name: 'Tooltip',
    render: (s) => s === '默认' || s === '聚焦' || s === '禁用' ? (
      <Tooltip open>
        <TooltipTrigger asChild>
          <Button size="sm" disabled={s === '禁用'}>{s === '禁用' ? '检测' : '侧栏'}</Button>
        </TooltipTrigger>
        <TooltipContent side="right">
          {s === '禁用' ? '先填写 API Key' : <>开关侧栏 <ShortcutKbd binding="mod+b" /></>}
        </TooltipContent>
      </Tooltip>
    ) : s === '悬停' ? '指针移入 tooltip 不保持（纯提示）' : '不适用',
  },
  {
    id: 'A16', name: '菜单 Menu',
    render: (s) => s === '默认' || s === '悬停' || s === '禁用' || s === '错误' || s === '加载' ? (
      <div className="h-[250px]">
      <DropdownMenu open modal={false}>
        <DropdownMenuTrigger asChild><Button size="xs">{s}</Button></DropdownMenuTrigger>
        <DropdownMenuContent side="bottom" align="start" onCloseAutoFocus={(e) => e.preventDefault()}>
          {s === '错误' ? (
            <DropdownMenuError message="成员列表读取失败" retryLabel="重试" onRetry={() => {}} />
          ) : s === '加载' ? (
            <DropdownMenuSkeleton label="加载中" />
          ) : (
            <>
              <DropdownMenuLabel>点名</DropdownMenuLabel>
              <DropdownMenuGroup>
                <DropdownMenuItem icon={<Icon name="agent" size={16} />} description="设计" data-force={s === '悬停' ? 'hover' : undefined} className="in-data-[force~=hover]:bg-hover">小设</DropdownMenuItem>
                <DropdownMenuItem icon={<Icon name="sleep" size={16} />} description={s === '禁用' ? '休眠' : '开发'} disabled={s === '禁用'}>小开</DropdownMenuItem>
                <DropdownMenuItem tall icon={<Icon name="shield" size={16} />} description="每一步都等你批准">谨慎</DropdownMenuItem>
              </DropdownMenuGroup>
              <DropdownMenuSeparator />
              <DropdownMenuFooter>↑↓ 选择 · ↵ 插入 · Esc 关闭</DropdownMenuFooter>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
      </div>
    ) : '按下 / 聚焦同悬停底（键盘高亮 = data-highlighted）',
  },
  {
    id: 'A17', name: '模态 Dialog',
    render: (s) => s === '默认' || s === '聚焦' || s === '加载' || s === '错误' || s === '禁用' ? (
      <StaticModal state={s} />
    ) : '按钮各自悬停 / 按下',
  },
]

const SWATCH_ROWS = [
  ['page', 'canvas', 'surface', 'hover-2', 'line-strong', 'ink'],
  ['accent', 'accent-tint', 'green', 'orange', 'red', 'violet'],
]

/** index#tokens：同一组语义 token 在两套主题下并排。 */
function TokenPane({ theme }: { theme: 'light' | 'dark' }) {
  return (
    <div data-theme={theme} className="flex-1 rounded-[var(--r-panel)] bg-page p-5 text-ink shadow-[var(--shadow-card)]">
      <div className="mb-3 text-[13px] font-semibold">{theme === 'light' ? '浅色' : '深色'}</div>
      {SWATCH_ROWS.map((row, i) => (
        <div key={i} className={i ? 'mt-3' : ''}>
          <div className="grid grid-cols-6 gap-2">
            {row.map((t) => <div key={t} className="h-10 rounded-[8px] shadow-[inset_0_0_0_1px_var(--line)]" style={{ background: `var(--${t})` }} />)}
          </div>
          <div className="mt-1 grid grid-cols-6 gap-2 font-mono text-[10.5px] text-ink-3">{row.map((t) => <span key={t}>{t}</span>)}</div>
        </div>
      ))}
      <div className="mt-4 flex flex-wrap items-center gap-1.5">
        <Button size="sm" variant="primary">主操作</Button><Button size="sm">次操作</Button><Button size="sm" variant="quiet">安静</Button>
        <Badge variant="ok">通过</Badge><Badge variant="warn">待决</Badge><Badge variant="err">失败</Badge>
      </div>
    </div>
  )
}

const TYPE_ROWS: [string, string, string, string][] = [
  ['页面标题 22/500', 'text-[22px] font-medium tracking-[-.02em]', '团队已就位，要从哪里开始？', '首屏问候、空态'],
  ['分区标题 18/600', 'text-[18px] font-semibold', '项目说明', '向导、设置'],
  ['卡片标题 14/600', 'text-[14px] font-semibold', '「实现」阶段等你盖章', '待决卡、图表卡'],
  ['正文 14/400 · 1.6', 'text-[14px] leading-[1.6]', '列表页和本地存储完成，3 条测试通过。', '消息、说明'],
  ['界面 13/400', 'text-[13px]', '搜索与命令 · 待决 · 产物', '导航、按钮、行'],
  ['辅助 12/400 ink-3', 'text-[12px] text-ink-3', '权限询问 · 联网 · 前台运行', '元信息'],
  ['等宽 12/400', 'font-mono text-[12px]', 'npx playwright install chromium', '路径、命令、数值'],
]

/** index#type：字号层级、圆角、层级、动效。 */
function TypeStage() {
  return (
    <div className="rounded-[var(--r-panel)] bg-canvas p-6 shadow-[inset_0_0_0_1px_var(--line-soft)]">
      <Card className="px-5 py-1">
        {TYPE_ROWS.map(([spec, cls, sample, use], i) => (
          <div key={spec} className={`grid grid-cols-[160px_1fr_auto] items-center gap-4 py-2.5 ${i < TYPE_ROWS.length - 1 ? 'border-b border-line-soft' : ''}`}>
            <span className="font-mono text-[12px] text-ink-3">{spec}</span>
            <span className={cls}>{sample}</span>
            <span className="text-[12px] text-ink-3">{use}</span>
          </div>
        ))}
      </Card>
      <div className="mt-4 grid grid-cols-3 gap-5">
        <Card className="p-4">
          <div className="mb-2 text-[12px] text-ink-3">圆角</div>
          <div className="flex items-center gap-3">
            {['rounded-[var(--r-xs)] w-10', 'rounded-[var(--r-card)] w-10', 'rounded-[var(--r-panel)] w-10', 'rounded-full w-16 !h-7'].map((c) => (
              <div key={c} className={`h-10 bg-canvas shadow-[inset_0_0_0_1px_var(--line)] ${c}`} />
            ))}
          </div>
          <div className="mt-2 text-[12px] text-ink-3">6 chip · 10 卡片 · 14 面板 · pill 按钮</div>
        </Card>
        <Card className="p-4">
          <div className="mb-2 text-[12px] text-ink-3">层级（阴影）</div>
          <div className="flex items-center gap-3">
            <Card className="h-10 w-14" /><Card surface="raised" className="h-10 w-14" /><Card surface="overlay" className="h-10 w-14" />
          </div>
          <div className="mt-2 text-[12px] text-ink-3">card · raised · overlay</div>
        </Card>
        <Card className="p-4">
          <div className="mb-2 text-[12px] text-ink-3">动效</div>
          <span className="inline-flex items-center gap-2 text-[13px] text-ink-2"><PixelGrid /><Shimmer>像素格 + shimmer</Shimmer></span>
          <div className="mt-2 text-[12px] text-ink-3">只用 transform / opacity；尊重 reduced-motion</div>
        </Card>
      </div>
    </div>
  )
}

type SpecimenTheme = 'light' | 'dark' | 'system'
const systemTheme = () => (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')

/** 只改当前文档的 data-theme，不写用户存的主题偏好（标本页只是看，不该改设置）。 */
function ThemeBar() {
  const [pref, setPref] = useState<SpecimenTheme>('system')
  useEffect(() => {
    const apply = () => { document.documentElement.dataset.theme = pref === 'system' ? systemTheme() : pref }
    apply()
    if (pref !== 'system') return
    const mq = matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', apply)
    return () => mq.removeEventListener('change', apply)
  }, [pref])
  return (
    <Segmented value={pref} onValueChange={(v) => setPref(v as SpecimenTheme)} aria-label="主题">
      <SegmentedItem value="light">浅</SegmentedItem>
      <SegmentedItem value="dark">深</SegmentedItem>
      <SegmentedItem value="system">跟随系统</SegmentedItem>
    </Segmented>
  )
}

function LiveConfirm() {
  const [open, setOpen] = useState(false)
  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <AlertDialogTrigger asChild><Button size="sm">打开真实确认层</Button></AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogIcon tone="warn"><Icon name="warn" /></AlertDialogIcon>
          <AlertDialogTitle>回退到「设计」阶段？</AlertDialogTitle>
        </AlertDialogHeader>
        <AlertDialogDescription>之后阶段的产物会标为已作废。</AlertDialogDescription>
        <AlertDialogFooter>
          <AlertDialogCancel size="sm">取消</AlertDialogCancel>
          <AlertDialogAction size="sm" variant="danger" onClick={() => setOpen(false)}>回退</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default function Specimen() {
  return (
    <div className="h-full overflow-auto bg-page font-sans text-[14px] leading-normal text-ink antialiased">
      <header className="sticky top-0 z-10 flex items-center gap-3 border-b border-line bg-page px-8 py-4">
        <span className="grid size-7 place-items-center rounded-[8px] bg-ink text-page"><Icon name="hex" /></span>
        <h1 className="text-[20px] font-semibold tracking-[-.01em]">组件标本 · components A1–A17</h1>
        <span className="flex-1" />
        <LiveConfirm />
        <ThemeBar />
      </header>
      <section id="tokens" className="px-8 pt-8">
        <h2 className="mb-3 text-[14px] font-semibold">色板（深浅并排） <span className="font-normal text-ink-3">index#tokens</span></h2>
        <div className="flex gap-4"><TokenPane theme="light" /><TokenPane theme="dark" /></div>
      </section>
      <section id="type" className="px-8 pt-8">
        <h2 className="mb-3 text-[14px] font-semibold">字号层级与形制 <span className="font-normal text-ink-3">index#type</span></h2>
        <TypeStage />
      </section>
      <h2 className="px-8 pt-8 text-[14px] font-semibold">状态矩阵 <span className="font-normal text-ink-3">components A1–A17 · 顶部切主题看深色</span></h2>
      <table className="w-full border-separate border-spacing-x-4 border-spacing-y-0 px-4">
        <thead>
          <tr className="text-left text-[12px] text-ink-3">
            <th className="w-40 py-3 font-normal">组件</th>
            {STATES.map((s) => <th key={s} className="min-w-[260px] py-3 font-normal">{s}</th>)}
          </tr>
        </thead>
        <tbody>
          {ROWS.map((r) => (
            <tr key={r.id} id={r.id} className="align-top">
              <td className="border-t border-line py-4 text-[13px]">
                <div className="font-medium">{r.name}</div>
                <div className="font-mono text-[11px] text-ink-3">{r.id}</div>
              </td>
              {STATES.map((s) => (
                <td key={s} className="border-t border-line py-4"><Cell state={s}>{r.render(s)}</Cell></td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
