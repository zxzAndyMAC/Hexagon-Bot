import { DesktopPermissions } from './components/DesktopPermissions'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import './i18n'
import { TIMELINE_JUMP_EVENT } from './timelineJump'
import { LOCATE_PENDING_EVENT } from './managedNext'

import { Timeline } from './components/Timeline'
import { Sidebar } from './components/shell/Sidebar'
import { SidebarHandle } from './components/shell/SidebarHandle'
import { Header } from './components/shell/Header'
import { Canvas } from './components/shell/Canvas'
import { CanvasHandle } from './components/shell/CanvasHandle'
import { UsageView } from './components/shell/UsageView'
import { useShellLayout } from './components/shell/useShellLayout'
import { mainView } from './layout'
import { FlowDialog } from './components/shell/FlowDialog'
import { CapBanner } from './components/shell/UsageEntry'
import { TooltipProvider } from './components/ui/tooltip'
import { Composer } from './components/Composer'
import { SettingsPage, type Section } from './components/SettingsPage'
import { Launcher } from './components/Launcher'
import { CommandPalette } from './components/CommandPalette'
import { api, errText, onTurnDelta, onToolOutput } from './api'
import { confirmIntakeDraft } from './intakeDraft'
import { useUiStore } from './store'
import { PendingDock } from './components/PendingDock'
import { ManagedStrip } from './components/ManagedStrip'
import { Icon } from './components/Icon'
import { usePendingKeys } from './decisions'
import { SwitchWaitBanner } from './components/SwitchWaitBanner'
import { bindingFor, matches } from './keymap'
import { closeOtherTabs, closeTabsToRight, copyTabPath, tabPath, stepTab } from './tabActions'
import { copyProjectPath, insertPathToComposer, revealPath, treeRowTarget } from './fileActions'
import { treeInsertPath } from './composerAtoms'
import { pauseDesktop, resumeDesktop } from './desktopPause'
import { dispatchMessageAction, messageTarget } from './messageActions'
import { changeMemberAvatar, copyMemberName, memberRowTarget, mentionMember, openMemberDetail, restoreMemberAvatar } from './memberActions'
import { SessionRecoveryStrip } from './components/SessionRecoveryStrip'


export default function App() {
  const { t } = useTranslation()
  const refresh = useUiStore((s) => s.refresh)
  const refreshFast = useUiStore((s) => s.refreshFast)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const settingsOpenRef = useRef(false)
  useEffect(() => { settingsOpenRef.current = settingsOpen }, [settingsOpen])
  const [computerSettings, setComputerSettings] = useState<Section>('general')
  const [paletteOpen, setPaletteOpen] = useState(false)
  const flowDialogOpen = useUiStore((s) => s.flowDialogOpen)
  // ADR 0088 / 0090（票 03）：主区视图三态——对话 / 画布全宽 / 用量详情；后两种时对话与输入条一起让出主区。
  const view = useUiStore(mainView)
  const shell = useShellLayout()
  const hasTabs = useUiStore((s) => s.tabs.length > 0)
  const canvasOpen = useUiStore((s) => s.canvasOpen)
  const focusMode = useUiStore((s) => s.focusSnapshot != null)
  // 启动闸：null=未查，false=未开项目→启动页，true=工作台。mock 恒 true。
  const [projectOpen, setProjectOpen] = useState<boolean | null>(null)
  const projectRoot = useUiStore(s => s.projectRoot)
  const projectGeneration = useUiStore(s => s.projectGeneration)
  const { onKey } = usePendingKeys()
  // 票 09（D17，interaction §6）：进设置也过离开守卫——设置页盖住工作台时编辑器不卸载（Benchmark I1），
  // 但负责人看不见画布里的脏稿，可能就此忘掉；先三选一再进。取消 = 留在工作台。
  const enterSettings = useCallback(async (section?: Section) => {
    const s = useUiStore.getState()
    if (Object.values(s.fileEdits).some((edit) => edit.dirty) && !await s.confirmFileEdits()) return
    if (section) setComputerSettings(section)
    setSettingsOpen(true)
  }, [])

  useEffect(() => {
    api.projectOpen().then(setProjectOpen)
  }, [])

  useEffect(() => {
    const openModels = () => { if (projectOpen === true) void enterSettings('team') }
    const openComputer = () => { if (projectOpen === true) void enterSettings('perms') }
    const openSection = (event: Event) => {
      if (projectOpen !== true) return
      const section = (event as CustomEvent<unknown>).detail
      // review R2：托管报告行内动作还会指到 MCP / 技能分区（core managed/next.rs 的封闭表）。
      // owner-followups-1015 #15：发布卡 / ⌘K「发布…」写因 / 托管报告「配置发布目标」→ 设置·发布。
      if (section !== 'models' && section !== 'usage' && section !== 'mcp' && section !== 'skills' && section !== 'release') return
      void enterSettings(section)
    }
    // Ticket 05: a report entry asked for its timeline row; the timeline jumps.
    // review R2: locating a pending card also needs the workbench (the dock lives there).
    const toTimeline = () => setSettingsOpen(false)
    window.addEventListener('hexagon:computer-model-settings', openModels)
    window.addEventListener('hexagon:computer-settings', openComputer)
    window.addEventListener('hexagon:open-settings-section', openSection)
    window.addEventListener(TIMELINE_JUMP_EVENT, toTimeline)
    window.addEventListener(LOCATE_PENDING_EVENT, toTimeline)
    return () => {
      window.removeEventListener(TIMELINE_JUMP_EVENT, toTimeline)
      window.removeEventListener(LOCATE_PENDING_EVENT, toTimeline)
      window.removeEventListener('hexagon:computer-model-settings', openModels)
      window.removeEventListener('hexagon:computer-settings', openComputer)
      window.removeEventListener('hexagon:open-settings-section', openSection)
    }
  }, [projectOpen, enterSettings])

  // ui-audit 票 01/02：界面作用域同步进 store——裁决快捷键据此放行/拦截。
  // palette 可在设置页之上再开一层，优先级 palette > settings。
  useEffect(() => {
    useUiStore.getState().setModalScope(paletteOpen ? 'palette' : settingsOpen ? 'settings' : flowDialogOpen ? 'flow' : 'workbench')
  }, [settingsOpen, paletteOpen, flowDialogOpen])

  // 票 17：工作台已经在了再分析。不等这条命令，输入框不被它挡住。
  useEffect(() => {
    if (projectOpen !== true || !projectRoot || projectGeneration === null) return
    let cancel = false
    void api.runOpeningIntake().then(() => {
      if (!cancel) void useUiStore.getState().refreshFast()
    }).catch((e) => {
      if (!cancel) useUiStore.getState().pushToast(errText(e), 'err')
    })
    return () => { cancel = true }
  }, [projectOpen, projectRoot, projectGeneration])

  // 进工作台不等 MCP 握手。有服务在 starting 时停输入，状态轮询里更新。
  useEffect(() => {
    if (projectOpen !== true) return
    let stop = false
    let toldDown = false
    const tick = async () => {
      try {
        const rows = await api.mcpServices()
        if (stop) return
        const pending = rows.some((r) => r.status === 'starting')
        useUiStore.setState({ mcpPending: pending })
        if (!pending && !toldDown) {
          const down = rows.filter((r) => r.status === 'down')
          if (down.length) {
            toldDown = true
            useUiStore.getState().pushToast(
              down.map((r) => `${r.name}: ${r.error ?? 'handshake failed'}`).join('\n'),
              'err',
            )
          }
        }
        if (pending && !stop) setTimeout(tick, 400)
      } catch {
        if (!stop) useUiStore.setState({ mcpPending: false })
      }
    }
    void tick()
    return () => { stop = true }
  }, [projectOpen])

  useEffect(() => {
    if (projectOpen !== true) return
    refresh()
    // 票 07：轮询收窄到快通道（stages+pending+timeline 增量，稳态 3 invoke）；
    // 慢通道（usage/artifacts/avatars）由 invalidate 失效标签驱动。
    const iv = setInterval(refreshFast, 2000)
    return () => clearInterval(iv)
  }, [refresh, refreshFast, projectOpen])

  // 票 03：回合流式 delta 订阅（Tauri 事件 → store 瞬时缓冲；
  // 浏览器 dev 无推送通道，onTurnDelta 返回 no-op）。
  useEffect(() => {
    if (projectOpen !== true || !projectRoot || projectGeneration === null) return
    let un: (() => void) | undefined
    let cancelled = false
    onTurnDelta((d) => useUiStore.getState().applyDelta(d)).then((u) => {
      if (cancelled) u()
      else un = u
    })
    return () => { cancelled = true; un?.() }
  }, [projectOpen, projectRoot, projectGeneration])

  // exec-cards 票 04：bash 输出流订阅——同 turn-delta 纪律（瞬时通道）。
  useEffect(() => {
    if (projectOpen !== true || !projectRoot || projectGeneration === null) return
    let un: (() => void) | undefined
    let cancelled = false
    onToolOutput((d) => useUiStore.getState().applyToolOutput(d)).then((u) => {
      if (cancelled) u()
      else un = u
    })
    return () => { cancelled = true; un?.() }
  }, [projectOpen, projectRoot, projectGeneration])

  // 票 16（方向卡 1）：失焦自动值守——blur 持续 60s 才 owner_away
  //（短抖动不误触发），回焦即 owner_back（markBack 内有 away 守卫）。
  useEffect(() => {
    if (projectOpen !== true) return
    let timer: ReturnType<typeof setTimeout> | undefined
    const onBlur = () => {
      timer = setTimeout(() => void useUiStore.getState().markAway(), 60_000)
    }
    const onFocus = () => {
      clearTimeout(timer)
      void useUiStore.getState().markBack()
    }
    window.addEventListener('blur', onBlur)
    window.addEventListener('focus', onFocus)
    return () => {
      clearTimeout(timer)
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('focus', onFocus)
    }
  }, [projectOpen])

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      // Ticket 07: Settings unmounts Composer, but cannot remove the owner's
      // only explicit computer stop key. It also works from focused inputs.
      if (matches(e, bindingFor('desktopPause'))) {
        e.preventDefault()
        if (!e.repeat && projectOpen === true) void pauseDesktop()
        return
      }
      if (!e.defaultPrevented && matches(e, bindingFor('desktopResume'))) {
        e.preventDefault()
        if (!e.repeat && projectOpen === true) void resumeDesktop()
        return
      }
      // Owner issue19: retained keymap actions now navigate to Settings.
      if (matches(e, bindingFor('desktopPanel')) || matches(e, bindingFor('browserPanel'))) {
        e.preventDefault()
        if (projectOpen === true) void enterSettings('perms')
        return
      }
      // 保存要在 Monaco 自己的 textarea 里也生效，所以赶在输入框豁免之前。
      if (matches(e, bindingFor('saveFile'))) {
        e.preventDefault()
        useUiStore.getState().requestSaveFile()
        return
      }
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      if (matches(e, bindingFor('commandPalette'))) {
        e.preventDefault()
        setPaletteOpen((v) => !v)
        return
      }
      if (matches(e, bindingFor('settings'))) {
        e.preventDefault()
        // 启动页/向导期 settingsOpen 是死状态——渲染先落 Launcher 分支，
        // 原地 toggle 只留 armed 标记，项目一开设置页误弹（owner 2026-09-30）。
        // 该期间改发 hexagon:open-settings，由 Launcher 的设置覆盖层接。
        if (projectOpen === true) {
          if (settingsOpenRef.current) setSettingsOpen(false)
          else void enterSettings()
        }
        else if (projectOpen === false) void api.openSettings()
        return
      }
      if (matches(e, bindingFor('focusComposer'))) {
        e.preventDefault()
        const s = useUiStore.getState()
        if (s.modalScope !== 'workbench') return
        // ADR 0090（票 03，interaction §3）：画布全宽 / 用量详情时先回到对话，再聚焦输入条。
        s.showConversation()
        // Owner 2026-10-06: reveal the conversation before focusing its hidden input.
        requestAnimationFrame(() => {
          const current = useUiStore.getState()
          if (current.modalScope === 'workbench' && mainView(current) === 'chat') {
            document.getElementById('composer-input')?.focus()
          }
        })
        return
      }
      // ADR 0088（票 02）：⌘B 由「开关右栏」改为「开关左侧栏」；⌥⌘B 开关画布（票 03 接入画布本体）。
      if (matches(e, bindingFor('toggleSidebar'))) {
        e.preventDefault()
        if (projectOpen === true) useUiStore.getState().toggleSidebar()
        return
      }
      if (matches(e, bindingFor('toggleCanvas'))) {
        e.preventDefault()
        if (projectOpen === true) useUiStore.getState().toggleCanvas()
        return
      }
      // 票 03（R-04）：分屏撤掉，⌘\ 改为画布全宽 ↔ 并排；R-06：⌘. 专注模式。
      // 设置页 / 命令面板盖着时不动看不见的布局。
      if (matches(e, bindingFor('canvasFullWidth'))) {
        e.preventDefault()
        if (projectOpen === true && useUiStore.getState().modalScope === 'workbench') useUiStore.getState().toggleCanvasFull()
        return
      }
      if (matches(e, bindingFor('focusMode'))) {
        e.preventDefault()
        if (projectOpen === true && useUiStore.getState().modalScope === 'workbench') useUiStore.getState().toggleFocusMode()
        return
      }
      // ADR 0088（票 03）：⌘W 关画布当前页签；画布看不见时不关（时间线不再是页签，没有可关的）。
      if (matches(e, bindingFor('closeTab'))) {
        e.preventDefault()
        const s = useUiStore.getState()
        if (s.modalScope === 'workbench' && s.canvasOpen && !s.usageView && s.activeTab) s.closeTab(s.activeTab)
        return
      }
      // owner-followups-1015 #08（A16b 成员行 / 消息）：默认空绑定。消息动作作用于焦点（或选区）所在的消息，
      // 成员动作作用于焦点成员行；没有目标就只吞掉这次按键（已被负责人绑给这个动作）。
      for (const [action, verb] of [['copyMessage', 'copy'], ['quoteMessage', 'quote'], ['locateInRail', 'locate']] as const) {
        const b = bindingFor(action)
        if (!b || !matches(e, b)) continue
        e.preventDefault()
        const host = useUiStore.getState().modalScope === 'workbench' ? messageTarget() : null
        if (host) dispatchMessageAction(host, verb)
        return
      }
      for (const [action, run] of [['openMemberDetail', openMemberDetail], ['mentionMember', mentionMember], ['copyMemberName', copyMemberName],
        ['changeMemberAvatar', changeMemberAvatar], ['restoreMemberAvatar', restoreMemberAvatar]] as const) {
        const b = bindingFor(action)
        if (!b || !matches(e, b)) continue
        e.preventDefault()
        const id = useUiStore.getState().modalScope === 'workbench' ? memberRowTarget() : null
        if (id) void run(id)
        return
      }
      // owner-followups-1015 #05（A16b）：右键菜单动作的键位入口，默认空绑定（matches 对空串恒假）；同 ⌘W 只在画布可见时作用于当前页签。
      // owner-followups-1015 #06（A22「溢出」）：⌘⇧] / ⌘⇧[ 按可见顺序切页签；同 ⌘W 只在画布可见时作用。
      for (const [action, dir] of [['nextTab', 1], ['prevTab', -1]] as const) {
        if (!matches(e, bindingFor(action))) continue
        e.preventDefault()
        const s = useUiStore.getState()
        if (s.modalScope === 'workbench' && s.canvasOpen && !s.usageView) stepTab(dir)
        return
      }
      for (const [action, run] of [['closeOtherTabs', closeOtherTabs], ['closeTabsToRight', closeTabsToRight]] as const) {
        const b = bindingFor(action)
        if (!b || !matches(e, b)) continue
        e.preventDefault()
        const s = useUiStore.getState()
        if (s.modalScope === 'workbench' && s.canvasOpen && !s.usageView && s.activeTab) void run(s.activeTab)
        return
      }
      // owner-followups-1015 #07（A16b 文件树行）：路径动作（复制 / 插入到输入条 / 在访达中显示）的键位入口，
      // 默认空绑定；作用对象 = 焦点文件树行，否则画布可见时的当前文件 / 产物页签（#05 的 copyPath 同此扩展）。
      for (const action of ['copyPath', 'insertPath', 'revealPath'] as const) {
        const b = bindingFor(action)
        if (!b || !matches(e, b)) continue
        e.preventDefault()
        const s = useUiStore.getState()
        if (s.modalScope !== 'workbench') return
        const tab = s.canvasOpen && !s.usageView ? tabPath(s.tabs.find((x) => x.id === s.activeTab)) : null
        const row = treeRowTarget()
        const target = row ?? (tab ? { path: tab, kind: 'file' as const } : null)
        if (!target) return
        // 页签与文件树行都写绝对路径（票 29 页签、#30 文件树行；2026-10-20 负责人裁决两处统一）
        if (action === 'copyPath') void (row ? copyProjectPath(row.path) : copyTabPath(s.activeTab!))
        else if (action === 'revealPath') void revealPath(target.path)
        else {
          const path = treeInsertPath(target)
          if (path) insertPathToComposer(path)
        }
        return
      }
      // ADR 0069：退回/跳过/盖章不再有快捷键。最终验收留在待决卡。
      if (useUiStore.getState().modalScope === 'workbench') {
        // 票 12：mod+J 聚焦节点轨（键盘可达性入口）
        if (matches(e, bindingFor('nodeRail'))) { e.preventDefault(); useUiStore.getState().focusNodeRail(); return }
        if (matches(e, bindingFor('treeNewFile'))) { e.preventDefault(); useUiStore.getState().requestFileTree('new-file'); return }
        if (matches(e, bindingFor('treeNewFolder'))) { e.preventDefault(); useUiStore.getState().requestFileTree('new-folder'); return }
        if (matches(e, bindingFor('treeRefresh'))) { e.preventDefault(); useUiStore.getState().requestFileTree('refresh'); return }
        if (matches(e, bindingFor('confirmIntake')) && useUiStore.getState().intakeDraft) {
          e.preventDefault()
          void confirmIntakeDraft()
          return
        }
      }
      void onKey(e)
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onKey, projectOpen, enterSettings])

  // ui-audit 票 06（P2-14）：冷启动骨架屏——projectOpen 查询期间
  // 不再渲染纯白窗口（曾被当成崩溃）。
  if (projectOpen === null) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 14 }}>
        <span style={{ color: 'var(--accent)', display: 'inline-flex', animation: 'pulse-soft 1.6s infinite' }}>
          <Icon name="hex" size={30} />
        </span>
        <span className="dim3" style={{ fontSize: 12 }}>{t('app.loading')}</span>
      </div>
    )
  }
  if (!projectOpen) {
    return <Launcher onOpen={() => setProjectOpen(true)} />
  }
  // Benchmark I1/2026-10-06: unmounting the workbench for Settings lost
  // unsaved file text before the project-leave guard could see it. Keep the
  // editor panes; stop native previews by unmounting the side panel as before.
  // 票 03 起编辑器在画布里：画布进设置页时同样不卸载（只随工作台 display:none）。
  // ui-redesign 票 02 / 03（ADR 0088 / 0090）：左侧栏（B1）+ 分割线（B8）+ 主区（B6：头部 + 常驻对话 + 输入条）
  // + 分割线 + 右侧画布（B7）。窗口变窄时按 layout.ts resolveShell 让位，对话主区始终 ≥ 480。
  const openSettings = () => void enterSettings()
  const raiseCap = () => void enterSettings('usage')
  const narrowLead = shell.side !== 'expanded'
  return (
    <TooltipProvider>
      {settingsOpen && (
        <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
          <SettingsPage initialSection={computerSettings}
            onBack={() => { setSettingsOpen(false); setComputerSettings('general') }}
            onOpenUsageDetail={() => {
              useUiStore.getState().openUsageDetail()
              setSettingsOpen(false)
            }}
          />
          {paletteOpen && <CommandPalette onClose={() => setPaletteOpen(false)} />}
        </div>
      )}
    <div aria-hidden={settingsOpen || undefined} data-workbench data-main-view={view} data-focus-mode={focusMode || undefined}
      style={{ height: '100%', display: settingsOpen ? 'none' : 'flex' }}>
      {!settingsOpen && shell.side !== 'hidden' && (
        <>
          <Sidebar onSearch={() => setPaletteOpen(true)} onSettings={openSettings} onProjectClosed={() => setProjectOpen(false)}
            forceCollapsed={shell.sideForced} />
          <SidebarHandle forced={shell.sideForced} />
        </>
      )}
      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', background: 'var(--app-main)' }}>
        {!settingsOpen && <DesktopPermissions compact />}
        <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>
          {/* 主区对话（B6）：画布全宽 / 用量详情时随输入条一起收起——隐藏不卸载，草稿、附件、网页引用都保留（ADR 0090）。 */}
          <main hidden={view !== 'chat'} data-conversation
            style={{ flex: 1, minWidth: 0, display: view === 'chat' ? 'flex' : 'none', flexDirection: 'column',
              // 画布并排时对话列 560，否则 720（interaction §6）；时间线（票 05）读 --col-w
              ['--col-w' as string]: shell.canvas === 'side' ? '560px' : '720px' }}>
            <Header side={shell.side} />
            {/* 票 13：切换项目阻塞横条（头部下方，feedback#errors） */}
            <SwitchWaitBanner />
            {/* 票 07：对话列宽只由 --col-w 决定（时间线 .tl-col、恢复条、触顶横幅、输入条同读），主区不再加左右内距 */}
            <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
              {/* C12a：钉在对话列顶部、归来摘要（时间线顶部）上方；与时间线同列对齐（列宽规则在 timeline.css） */}
              <SessionRecoveryStrip />
              <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
                <Timeline />
              </div>
              {/* B4 触顶横幅：输入条正上方（interaction §10） */}
              <CapBanner onRaise={raiseCap} />
              {/* 票 08（D7）：托管生效时细条替坞占位；坞只在仍有负责人事项时同时出现 */}
              <ManagedStrip />
              {/* 票 04（ADR 0089，D6）：待决坞停在输入条正上方，不遮挡时间线；Tab 序 对话 → 坞 → 输入条 */}
              <PendingDock />
              <Composer />
            </div>
          </main>
          {view === 'usage' && <UsageView narrowLead={narrowLead} />}
          {shell.canvas === 'side' && <CanvasHandle width={shell.canvasWidth} max={shell.canvasMax} />}
          {(hasTabs || canvasOpen) && (
            <Canvas open={shell.canvas !== 'none'} mode={shell.canvas === 'none' ? 'side' : shell.canvas}
              width={shell.canvasWidth} narrowLead={narrowLead} />
          )}
        </div>
      </div>
      {!settingsOpen && <FlowDialog />}
      {!settingsOpen && paletteOpen && <CommandPalette onClose={() => setPaletteOpen(false)} onSettings={openSettings} />}
    </div>
    </TooltipProvider>
  )
}
