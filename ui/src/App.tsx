import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import './i18n'
import { TopBar } from './components/TopBar'
import { StageBar } from './components/StageBar'
import { Timeline } from './components/Timeline'
import { TabBar } from './components/TabBar'
import { ArtifactTab } from './components/ArtifactTab'
import { AgentTab } from './components/AgentTab'
import { UsageDetailTab } from './components/UsageDetailTab'
import { DiffView } from './components/DiffView'
import { parseUnifiedDiff } from './diff'
import { SidePanel } from './components/SidePanel'
import { Composer } from './components/Composer'
import { SettingsPage } from './components/SettingsPage'
import { Launcher } from './components/Launcher'
import { CommandPalette } from './components/CommandPalette'
import { api, onTurnDelta } from './api'
import { useUiStore } from './store'
import { PendingCards } from './components/PendingCards'
import { Icon } from './components/Icon'
import { usePendingKeys } from './decisions'
import { bindingFor, matches } from './keymap'
import { runStageOp } from './stageops'

export default function App() {
  const { t } = useTranslation()
  const refresh = useUiStore((s) => s.refresh)
  const refreshFast = useUiStore((s) => s.refreshFast)
  const pending = useUiStore((s) => s.pending)
  const timeline = useUiStore((s) => s.timeline)
  const pendingH = useUiStore((s) => s.pendingH)
  const pendingCollapsed = useUiStore((s) => s.pendingCollapsed)
  const setPendingH = useUiStore((s) => s.setPendingH)
  const setPendingCollapsed = useUiStore((s) => s.setPendingCollapsed)
  const zoneRef = useRef<HTMLDivElement>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  // 启动闸：null=未查，false=未开项目→启动页，true=工作台。mock 恒 true。
  const [projectOpen, setProjectOpen] = useState<boolean | null>(null)
  const { onKey } = usePendingKeys()

  useEffect(() => {
    api.projectOpen().then(setProjectOpen)
  }, [])

  // ui-audit 票 01/02：界面作用域同步进 store——裁决快捷键据此放行/拦截。
  // palette 可在设置页之上再开一层，优先级 palette > settings。
  useEffect(() => {
    useUiStore.getState().setModalScope(paletteOpen ? 'palette' : settingsOpen ? 'settings' : 'workbench')
  }, [settingsOpen, paletteOpen])

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
    if (projectOpen !== true) return
    let un: (() => void) | undefined
    onTurnDelta((d) => useUiStore.getState().applyDelta(d)).then((u) => {
      un = u
    })
    return () => un?.()
  }, [projectOpen])

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
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      if (matches(e, bindingFor('commandPalette'))) {
        e.preventDefault()
        setPaletteOpen((v) => !v)
        return
      }
      if (matches(e, bindingFor('settings'))) {
        e.preventDefault()
        setSettingsOpen((v) => !v)
        return
      }
      if (matches(e, bindingFor('focusComposer'))) {
        e.preventDefault()
        document.getElementById('composer-input')?.focus()
        return
      }
      if (matches(e, bindingFor('toggleRail'))) {
        e.preventDefault()
        useUiStore.getState().setRailOpen(!useUiStore.getState().railOpen)
        return
      }
      if (matches(e, bindingFor('splitEditor'))) {
        e.preventDefault()
        useUiStore.getState().setSplitOpen(!useUiStore.getState().splitOpen)
        return
      }
      if (matches(e, bindingFor('closeTab'))) {
        e.preventDefault()
        const s = useUiStore.getState()
        s.closeTab(s.activeTab) // timeline tab 在 closeTab 内被拦
        return
      }
      // ui-audit 票 03：stage 操作绑定（ADR 0056-2）。与裁决键同样
      // 吃作用域守卫——设置页/palette 打开时不许动阶段。
      if (useUiStore.getState().modalScope === 'workbench') {
        if (matches(e, bindingFor('stageRewind'))) { e.preventDefault(); runStageOp('rewind'); return }
        if (matches(e, bindingFor('stageSkip'))) { e.preventDefault(); runStageOp('skip'); return }
        if (matches(e, bindingFor('stageStamp'))) { e.preventDefault(); runStageOp('stamp'); return }
        // 票 12：mod+J 聚焦节点轨（键盘可达性入口）
        if (matches(e, bindingFor('nodeRail'))) { e.preventDefault(); useUiStore.getState().focusNodeRail(); return }
      }
      void onKey(e)
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onKey])

  // ui-audit 票 06（P2-14）：冷启动骨架屏——projectOpen 查询期间
  // 不再渲染纯白窗口（曾被当成崩溃）。
  if (projectOpen === null) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 14 }}>
        <span style={{ color: 'var(--accent)', display: 'inline-flex', animation: 'pulse-amber 1.6s infinite' }}>
          <Icon name="hex" size={30} />
        </span>
        <span className="dim3" style={{ fontSize: 12 }}>{t('app.loading')}</span>
      </div>
    )
  }
  if (!projectOpen) {
    return <Launcher onOpen={() => setProjectOpen(true)} />
  }
  // 设置整页：工作台整体换掉（票 29），arrow-left 返回
  if (settingsOpen) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <SettingsPage
          onBack={() => setSettingsOpen(false)}
          // ui-audit-2 票 05：设置-用量「详情」= 回工作台并开用量明细 tab
          onOpenUsageDetail={() => {
            useUiStore.getState().openTab({ id: 'usage', kind: 'usage', title: t('usage.detail') })
            setSettingsOpen(false)
          }}
        />
        {paletteOpen && <CommandPalette onClose={() => setPaletteOpen(false)} />}
      </div>
    )
  }

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <TopBar
        onSettings={() => setSettingsOpen(true)}
        onProjectClosed={() => setProjectOpen(false)}
      />
      <StageBar />
      <div style={{ flex: 1, minHeight: 0, display: 'flex', gap: 10, padding: '10px 14px' }}>
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          {/* ui-audit 票 11（P2-11）：待决区高度内容感知——≤2 卡自适应
              不圈地；积压封顶内滚；时间线近空时放宽上限多露卡。
              workbench-polish 04（owner）：分隔条可上下拖动手调高度，
              拖过收拢阈吸顶隐藏（TopBar 待决徽标点回）；手调高度持久化。 */}
          <div
            id="pending-zone"
            ref={zoneRef}
            style={{
              display: pendingCollapsed ? 'none' : undefined,
              height: pendingH ?? undefined,
              maxHeight: pendingH != null ? 'none' : pending.length <= 2 ? 'max-content' : timeline.length < 20 ? 340 : 220,
              overflowY: 'auto',
              flexShrink: 0,
            }}
          >
            <PendingCards />
          </div>
          {pending.length > 0 && (
            <div
              className={`zone-splitter${pendingCollapsed ? ' collapsed' : ''}`}
              title={t('zone.resizeHint')}
              onMouseDown={(e) => {
                e.preventDefault()
                // 收起态下 zone display:none 无盒——锚点改取把手自身顶缘；
                // grabOff 减手在把手内的偏移，起手不跳变。
                const handleTop = e.currentTarget.getBoundingClientRect().top
                const grabOff = e.clientY - handleTop
                const top = pendingCollapsed || !zoneRef.current
                  ? handleTop
                  : zoneRef.current.getBoundingClientRect().top
                document.body.style.userSelect = 'none'
                const move = (ev: MouseEvent) => {
                  const h = ev.clientY - grabOff - top
                  if (h < 56) { setPendingCollapsed(true); return } // 收拢阈：拖到顶→吸顶
                  setPendingCollapsed(false)
                  setPendingH(Math.min(h, window.innerHeight * 0.6))
                }
                const up = () => {
                  document.body.style.userSelect = ''
                  window.removeEventListener('mousemove', move)
                  window.removeEventListener('mouseup', up)
                }
                window.addEventListener('mousemove', move)
                window.addEventListener('mouseup', up)
              }}
            />
          )}
          <TabBar />
          <CenterPanes />
          <Composer />
        </div>
        <SidePanel />
      </div>
      {paletteOpen && <CommandPalette onClose={() => setPaletteOpen(false)} />}
    </div>
  )
}

function TabContent({ tab }: { tab: ReturnType<typeof useUiStore.getState>['tabs'][number] }) {
  switch (tab.kind) {
    case 'artifact':
      return <ArtifactTab path={tab.path!} />
    case 'agent':
      return <AgentTab agentId={tab.agentId!} />
    case 'usage':
      return <UsageDetailTab />
    case 'diff':
      return (
        <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
          <div className="row-line mono" style={{ padding: '8px 14px', fontSize: 12, fontWeight: 560, display: 'flex', alignItems: 'center', gap: 6 }}>
            <Icon name="diff" size={12} /> {tab.title}
          </div>
          <DiffView ops={parseUnifiedDiff(tab.patchText ?? '')} />
        </div>
      )
    default:
      return <Timeline />
  }
}

function CenterPanes() {
  const { tabs, activeTab, splitOpen } = useUiStore()
  const active = tabs.find((t) => t.id === activeTab) ?? tabs[0]
  // 分屏右栏默认给「另一个最近 tab」；本地状态记右栏选中
  // 票 15（P3）：splitId 上移 store——进出设置页后右栏选择保持。
  const splitId = useUiStore((s) => s.splitId)
  const setSplitId = useUiStore((s) => s.setSplitId)
  const others = tabs.filter((t) => t.id !== active.id)
  const splitTab = others.find((t) => t.id === splitId) ?? others[others.length - 1] ?? tabs[0]
  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        {tabs.map((t) => (
          <div key={t.id} style={{ display: t.id === active.id ? 'flex' : 'none', flex: 1, minHeight: 0, flexDirection: 'column' }}>
            <TabContent tab={t} />
          </div>
        ))}
      </div>
      {splitOpen && (
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', borderLeft: '1px solid var(--border-strong)' }}>
          <div className="row-line" style={{ display: 'flex', gap: 4, padding: '4px 8px', alignItems: 'center' }}>
            {tabs.map((t) => (
              <button
                key={t.id}
                className={`chip ${t.id === splitTab.id ? 'amber' : ''}`}
                style={{ cursor: 'pointer', fontSize: 10 }}
                onClick={() => setSplitId(t.id)}
              >
                {t.kind === 'agent' ? (t.role ?? t.title) : t.kind === 'timeline' ? <Icon name="list" size={10} /> : t.title}
              </button>
            ))}
          </div>
          <TabContent tab={splitTab} />
        </div>
      )}
    </div>
  )
}
