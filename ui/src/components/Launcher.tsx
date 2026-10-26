// 启动页（票 29；ui-redesign 票 10 按 entry#launcher / components E11 / interaction §11 重做）：
// 一句话开局 + 打开已有项目 + 空白新建 + 最近项目。未开项目时的全屏界面。
// 一句话写进向导草稿的 brief（WCAG 2.2 redundant-entry），「选择目录」chip 写进 dir；
// 向导第一步仍是模型服务商（放行条件不变）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, isTauri } from '../api'
import type { RecentProject } from '../gen/RecentProject'
import { useUiStore } from '../store'
import { recentTime } from '../recentTime'
import { cn } from '@/lib/utils'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty } from '@/components/ui/empty'
import { toastError } from '../toast'
import { IconButton } from '@/components/ui/icon-button'
import { Spinner } from '@/components/ui/spinner'
import { Wizard } from './Wizard'
import { Icon } from './Icon'
import { SettingsPage } from './SettingsPage'
import { dirName, type DraftSeed } from './wizard/draft'
import { OpenWithSubmenu } from './OpenWithMenu'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu'

async function pickDirectory(): Promise<string | null> {
  if (!isTauri) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const picked = await open({ directory: true })
  return typeof picked === 'string' ? picked : null
}

export function Launcher({ onOpen }: { onOpen: () => void }) {
  const { t, i18n } = useTranslation()
  const [recents, setRecents] = useState<RecentProject[]>([])
  const [loaded, setLoaded] = useState(false)
  // 向导：null = 启动页；seed 为启动页一句话带入的种子（空白新建不带）
  const [wizard, setWizard] = useState<{ seed?: DraftSeed } | null>(null)
  const [settings, setSettings] = useState(false)
  const [sentence, setSentence] = useState('')
  const [pickedDir, setPickedDir] = useState('')
  const [opening, setOpening] = useState<string | null>(null)
  const [now] = useState(() => Date.now())

  useEffect(() => {
    api.recentProjects().then(setRecents).catch(() => {}).finally(() => setLoaded(true))
    // 向导 keys 步等处的「去设置」与未开项目时的 ⌘, 都走 hexagon:open-settings
    // 广播（App.tsx 在项目未开时改发此事件）；收成 toggle 与工作台同键语义一致。
    const h = () => setSettings((v) => !v)
    window.addEventListener('hexagon:open-settings', h)
    return () => window.removeEventListener('hexagon:open-settings', h)
  }, [])

  const openDir = async (dir?: string) => {
    try {
      const d = dir ?? await pickDirectory()
      if (!d) return
      setOpening(d)
      await api.openRecent(d)
      onOpen()
    } catch (e) {
      // E11 错误态 / A18（票 13）：打开最近项目失败 toast 带「从最近项目移除」动作；
      // 从系统目录选择打开的不在最近列表里，不给这个动作。
      const recent = dir && recents.some((r) => r.dir === dir)
      toastError(e, recent ? { action: { label: t('feedback.removeRecent'), run: () => remove(dir) } } : {})
    } finally {
      setOpening(null)
    }
  }

  const remove = (dir: string) => {
    void api.removeRecent(dir)
      .then(() => api.recentProjects())
      .then(setRecents)
      .catch((x) => useUiStore.getState().pushToast(errText(x), 'err'))
  }

  const start = () => {
    const brief = sentence.trim()
    if (!brief) return
    setWizard({ seed: { brief, dir: pickedDir || undefined } })
  }

  // 设置与工作台内同一个 SettingsPage——不另开一套（术语表：设置页 §19）。
  // 盖在向导上而不替换：向导保持挂载，步序与草稿不丢；返回时 keys 步轮询自刷新。
  const body = wizard ? (
    <Wizard onDone={onOpen} onCancel={() => setWizard(null)} seed={wizard.seed} />
  ) : (
    // 2026-10-26 负责人反馈「向导第一步应固定不滚动，最近项目列表自己滚」：整页钉在视口高度，
    // 只有最近项目列表 overflow；窗口矮到放不下首屏（min-h）时才退回整页滚动。
    // 被否决：整页随列表长高再整页滚——项目多了一句话输入框会被推出视区。
    <div data-tauri-drag-region className="fixed inset-0 overflow-y-auto bg-[var(--app-main)] text-ink">
      <div className="flex h-full min-h-[480px] flex-col items-center px-4 pt-[110px] pb-16">
        <div className="grid size-10 place-items-center rounded-[12px] bg-ink text-page"><Icon name="hex" size={20} /></div>
        <h1 className="mt-[18px] mb-1.5 text-[28px] font-medium tracking-[-.02em]">{t('launch.heading')}</h1>
        <div className="text-[12.5px] text-ink-3">Hexagon-Bot · {t('launch.title')}</div>

        <div className="mt-6 w-[640px] max-w-full rounded-[18px] bg-surface shadow-[var(--shadow-raised)] focus-within:shadow-[var(--shadow-raised),0_0_0_4px_var(--accent-tint)]">
          <textarea
            autoFocus
            data-launch-prompt
            aria-label={t('launch.prompt')}
            placeholder={t('launch.prompt')}
            value={sentence}
            rows={2}
            onChange={(e) => setSentence(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault()
                start()
              }
            }}
            className="field-sizing-content block max-h-[200px] min-h-16 w-full resize-none bg-transparent px-4 pt-3.5 pb-1.5 text-[14px] leading-[1.55] text-ink outline-none placeholder:text-ink-3"
          />
          <div className="flex items-center gap-1.5 pt-1.5 pr-2 pb-2 pl-2.5">
            {isTauri && (
              <Button
                size="xs"
                variant="ghost"
                data-pick-dir
                title={pickedDir || undefined}
                onClick={() => { void pickDirectory().then((d) => { if (d) setPickedDir(d) }) }}
              >
                <Icon name="folder" size={14} />
                {pickedDir ? t('launch.pickedDir', { dir: dirName(pickedDir) }) : t('launch.pickDir')}
              </Button>
            )}
            <span className="flex-1" />
            <span className="text-[12px] text-ink-3">↵ {t('launch.start')}</span>
            <button
              type="button"
              data-launch-start
              aria-label={t('launch.start')}
              aria-disabled={!sentence.trim() || undefined}
              onClick={start}
              className={cn(
                'hx-press hx-focus grid size-8 place-items-center rounded-full',
                sentence.trim() ? 'cursor-pointer bg-ink text-page' : 'cursor-default bg-hover-2 text-ink-3',
              )}
            >
              <Icon name="send" size={16} />
            </button>
          </div>
        </div>

        <div className="mt-3 flex gap-1.5">
          {isTauri && (
            <Button size="sm" onClick={() => { void openDir() }}><Icon name="folder" size={14} />{t('launch.openExisting')}</Button>
          )}
          <Button size="sm" data-blank-new onClick={() => setWizard({})}><Icon name="plus" size={14} />{t('launch.blank')}</Button>
        </div>

        <div className="mt-7 flex min-h-0 w-[640px] max-w-full flex-1 flex-col">
          {loaded && recents.length === 0 ? (
            <Empty icon="folder" title={t('launch.emptyTitle')} desc={t('launch.emptyBody')}
              action={isTauri
                ? <Button size="sm" onClick={() => { void openDir() }}>{t('launch.openExisting')}</Button>
                : <Button size="sm" onClick={() => setWizard({})}>{t('launch.blank')}</Button>} />
          ) : recents.length > 0 && (
            <>
              <div className="flex flex-none px-3 pb-1.5 text-[12px] text-ink-3">
                {t('launch.recents')}<span className="flex-1" />{t('launch.recentCount', { count: recents.length })}
              </div>
              <ul role="list" data-launch-recents className="m-0 min-h-0 list-none overflow-y-auto rounded-[var(--r-panel)] bg-[var(--app-card)] p-1 shadow-[var(--shadow-card)]">
                {recents.map((r) => <RecentRow key={r.dir} r={r} when={recentTime(r.opened_at, now, i18n.language)} opening={opening === r.dir} onOpen={() => { void openDir(r.dir) }} onRemove={() => remove(r.dir)} />)}
              </ul>
            </>
          )}
        </div>
      </div>
    </div>
  )

  return (
    <>
      {body}
      {/* 2026-10-26 负责人反馈「启动页 / 向导时拖不动窗口」：Tauri 的裸 data-tauri-drag-region 只认
          「点在这个元素本身」（drag.js isDragRegion），而启动页内容列铺满整个底板、向导卡外只剩窄边，
          实际几乎没有可拖的像素。顶部补一条 44 高的拖动条（同设置页 h-11），z 16 与向导同层、靠 DOM
          序压在其上，低于右上设置钮 17。被否决：底板改 "deep"——向导正文与输入区的文字选择会变成拖窗。 */}
      <div data-tauri-drag-region data-launch-drag className="fixed inset-x-0 top-0 z-[16] h-11" />
      {/* 设置钮对启动页与向导全程恒显（owner 2026-09-30：向导期间也要能进
          设置）。z 17：压过全屏视图 16（向导）；设置覆盖 18 打开时盖住它（foundations §4，gap C5）。 */}
      <div className="fixed top-2 right-3.5 z-[17]">
        <IconButton label={t('launch.settings')} action="settings" data-launch-settings onClick={() => setSettings(true)}>
          <Icon name="settings" size={14} />
        </IconButton>
      </div>
      {settings && (
        <div style={{ position: 'fixed', inset: 0, zIndex: 18, background: 'var(--page)' }}>
          <SettingsPage onBack={() => setSettings(false)} projectless />
        </div>
      )}
    </>
  )
}

/// 最近项目行（E11 `rec`）：悬停 --hover 底、相邻分隔线隐去；目录已删（exists=false）是只读展示行——
/// open_recent 必报错，红字 + err chip，只剩 × 移除可操作。
function RecentRow({ r, when, opening, onOpen, onRemove }: {
  r: RecentProject
  when: string
  opening: boolean
  onOpen: () => void
  onRemove: () => void
}) {
  const { t } = useTranslation()
  const missing = !r.exists
  const row = 'group/rec flex items-center gap-3 rounded-[var(--r-card)] px-3 py-2.5 [&+&]:shadow-[0_-1px_0_var(--line-soft)] [&:hover+&]:shadow-none'
  const icon = (
    <span className={cn('grid size-[30px] flex-none place-items-center rounded-[var(--r-sm)]', missing ? 'bg-red-tint text-red' : r.mode === 'fastpath' ? 'bg-hover-2 text-ink' : 'bg-ink text-page')}>
      <Icon name={missing ? 'warn' : r.mode === 'fastpath' ? 'bolt' : 'hex'} size={12} />
    </span>
  )
  const info = (
    <div className="min-w-0 flex-1 text-left">
      <div className={cn('truncate text-[13px] font-medium', missing && 'text-red-ink')}>{r.name}</div>
      <div className={cn('truncate font-mono text-[12px]', missing ? 'text-red-ink' : 'text-ink-3')} title={r.dir}>{r.dir}</div>
    </div>
  )
  const removeBtn = (
    <IconButton size="sm" label={t('launch.remove')} onClick={(e) => { e.stopPropagation(); onRemove() }}>
      <Icon name="close" size={14} />
    </IconButton>
  )
  if (missing) {
    return (
      <li className={cn(row, 'recent-row opacity-80')} data-missing="1">
        {icon}{info}
        <Badge variant="err">{t('launch.missing')}</Badge>
        {removeBtn}
      </li>
    )
  }
  // #32（2026-10-21 负责人裁决「启动页最近项目右键加打开方式」）：项目级菜单 = 打开 · 打开方式 ▸。
  // 打开方式走 recent_* 命令：core 只认 app 自己 recents 记录里的目录为根，界面传的 dir 只是查表键（不在记录里即拒）。
  // 目录已删的行（上面 missing 分支）没有盘上目录，不挂菜单；没有路径类项（复制 / 揭示）——启动页没有项目根可依。
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <li className={cn(row, 'recent-row hx-press hx-focus cursor-pointer live:hover:bg-hover')} tabIndex={0} aria-busy={opening || undefined}
          onClick={onOpen}
          onKeyDown={(e) => { if (e.key === 'Enter' && e.target === e.currentTarget) { e.preventDefault(); onOpen() } }}
        >
          {icon}{info}
          <Badge>{t(`launch.mode_${r.mode}`)}</Badge>
          {when && <span className="flex-none text-[12px] text-ink-3">{when}</span>}
          {opening ? <Spinner /> : <span className="opacity-0 group-hover/rec:opacity-100 group-focus-within/rec:opacity-100">{removeBtn}</span>}
        </li>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem data-ctx-item="open" onSelect={onOpen}>{t('ctx.open')}</ContextMenuItem>
        <OpenWithSubmenu path={r.dir} recent />
      </ContextMenuContent>
    </ContextMenu>
  )
}
