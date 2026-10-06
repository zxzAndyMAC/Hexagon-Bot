import { PermissionScope } from './PermissionActions'
import { DesktopControlPanel } from './DesktopControlPanel'
import { BrowserControlPanel } from './BrowserControlPanel'
import { DesktopPauseButton } from './DesktopPauseButton'
import { localLogTime } from '../diagTime'
import { ProjectApprovalMode } from './ProjectApprovalMode'
import { DesktopPermissions } from './DesktopPermissions'
import type { ProjectSkillDocument } from '../gen/ProjectSkillDocument'
import { ExperienceEntries } from './ExperienceEntries'
// 设置整页（票 29）：工作台整体换成设置页，不是弹层。
// 左 nav 分区：通用/键盘/团队/模型与凭据/权限/技能/MCP 服务/自治/用量/日志/关于。
// 「日志」（diagnostic-records 票 01）：诊断开关在这里，通用页不再放。
// projectless（启动页「设置」入口，无项目上下文）：项目作用域分区渲染提示而
// 不发必败的 conn/wb IPC——否则每个分区都弹 internal toast（ui-audit-2 收口回归）。
import { useCallback, useEffect, useRef, useState } from 'react'
import { SettingsVirtualList } from './SettingsVirtualList'
import { LoadingState } from './LoadingState'
import { useTranslation } from 'react-i18next'
import i18n, { SUPPORTED, setLang, type Locale } from '../i18n'
import { useUiStore, type ThemePref } from '../store'
import { api, errText, isTauri, type DiagRecord, type ExtMcpRow, type ExtSkillRow, type McpEntryRow, type McpServiceRow, type PermissionRuleRow, type ProvidersView, type RoleDef, type RoleTemplate, type SkillRow } from '../api'
import { ACTIONS, bindingFor, conflictFor, formatBinding, isMac, matches, normalizeEvent, resetBinding, setBinding, type ActionId } from '../keymap'
import { PracticeGround } from './PracticeGround'
import { PromptsSection } from './PromptsSection'
import { Icon } from './Icon'
import { DataBoundary } from './DataBoundary'
import { ProviderManager } from './ProviderManager'
import { UsageTab } from './UsageTab'
import { RoleEditor, CreateRoleForm } from './RoleEditor'
import { EntityChips } from './EntityPicker'
import { Md, CodeBlock } from './Md'
import { Avatar } from './Avatar'
import { sharedSlots, slotLabel } from '../modelpick'

const LANG_NAMES: Record<string, string> = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  en: 'English',
  ja: '日本語',
  es: 'Español',
  pt: 'Português',
  fr: 'Français',
}

type Section = 'general' | 'keys' | 'team' | 'models' | 'perms' | 'skills' | 'mcp' | 'autonomy' | 'usage' | 'logs' | 'prompts' | 'about'
const SECTIONS: Section[] = ['general', 'keys', 'team', 'models', 'perms', 'skills', 'mcp', 'autonomy', 'usage', 'logs', 'prompts', 'about']
/// settings-3col 票 01：list|detail 分区放宽到 980（760 塞中列后详情太挤）；
/// 平铺分区保持 760 居中。
const WIDE_SECTIONS = new Set<Section>(['team', 'models', 'skills', 'mcp', 'prompts'])

/** settings-3col 票 01：Cherry 式「中列清单 | 右列详情」共享壳。
 *  中列 = 顶部搜索框 + 条目行（选中=左竖条+accent-soft 底）+ 底部动作行；
 *  右列 = children 自由渲染。模型/技能/MCP/团队四区共用，观感不漂移。 */
function ListDetail<T>({
  items,
  itemKey,
  filterText,
  renderItem,
  selected,
  onSelect,
  searchPlaceholder,
  actions,
  emptyText,
  virtualized = false,
  children,
}: {
  items: T[]
  itemKey: (t: T) => string
  filterText: (t: T) => string
  renderItem: (t: T) => React.ReactNode
  selected: string | null
  onSelect: (k: string | null) => void
  searchPlaceholder?: string
  actions?: React.ReactNode
  emptyText?: string
  virtualized?: boolean
  children: React.ReactNode
}) {
  const [q, setQ] = useState('')
  const shown = items.filter((it) =>
    !q || filterText(it).toLowerCase().includes(q.toLowerCase()),
  )
  return (
    <div style={{ display: 'flex', gap: 14, alignItems: 'stretch' }}>
      <div
        className="panel"
        style={{ width: 270, flexShrink: 0, display: 'flex', flexDirection: 'column', maxHeight: '72vh' }}
      >
        {searchPlaceholder && (
          <div style={{ padding: '8px 8px 0' }}>
            <input
              className="input"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder={searchPlaceholder}
              style={{ width: '100%' }}
            />
          </div>
        )}
        <div style={{ flex: 1, overflowY: 'auto', padding: '6px' }}>
          {shown.length === 0 ? (
            <div className="dim3" style={{ fontSize: 12, padding: '12px 8px' }}>{emptyText ?? '—'}</div>
          ) : virtualized && shown.length > 100 ? (
            <SettingsVirtualList key={q} items={shown} itemKey={itemKey} selected={selected} onSelect={onSelect} renderItem={renderItem} />
          ) : (
            shown.map((it) => {
              const k = itemKey(it)
              const sel = selected === k
              return (
                <div
                  key={k}
                  role="button"
                  tabIndex={0}
                  onClick={() => onSelect(sel ? null : k)}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(sel ? null : k) } }}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 8,
                    padding: '9px 10px', borderRadius: 8, cursor: 'pointer',
                    boxShadow: sel ? 'inset 2px 0 0 var(--accent)' : 'inset 2px 0 0 transparent',
                    background: sel ? 'var(--accent-soft)' : undefined,
                  }}
                >
                  {renderItem(it)}
                </div>
              )
            })
          )}
        </div>
        {actions && (
          <div style={{ borderTop: '1px solid var(--border)', padding: 8, display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            {actions}
          </div>
        )}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>{children}</div>
    </div>
  )
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 0', borderBottom: '1px solid var(--border)' }}>
      <div style={{ flex: 1 }}>
        <div>{label}</div>
        {hint && <div className="dim3" style={{ fontSize: 12, marginTop: 2 }}>{hint}</div>}
      </div>
      {children}
    </div>
  )
}

function KeyRow({ id, onChanged }: { id: ActionId; onChanged: () => void }) {
  const { t } = useTranslation()
  const [capturing, setCapturing] = useState(false)
  const [conflict, setConflict] = useState<ActionId | null>(null)
  const [, setTick] = useState(0)

  useEffect(() => {
    if (!capturing) return
    const h = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') { setCapturing(false); return }
      const b = normalizeEvent(e)
      if (!b) return // 纯修饰键，继续等
      const c = conflictFor(id, b)
      if (c) { setConflict(c); setCapturing(false); return }
      setBinding(id, b)
      setConflict(null)
      setCapturing(false)
      setTick((n) => n + 1)
      onChanged()
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [capturing, id, onChanged])

  const label = ACTIONS.find((a) => a.id === id)?.labelKey ?? id
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 0', borderBottom: '1px solid var(--border)' }}>
      <div style={{ flex: 1, fontSize: 13 }}>{t(label)}</div>
      {conflict && (
        <span style={{ color: 'var(--accent)', fontSize: 12 }}>
          {t('settings.keyConflict', {
            action: t(ACTIONS.find((a) => a.id === conflict)?.labelKey ?? conflict),
          })}
        </span>
      )}
      <button
        className="btn mono"
        style={{ minWidth: 90 }}
        onClick={() => { setConflict(null); setCapturing(true) }}
      >
        {capturing ? t('settings.pressKey') : formatBinding(bindingFor(id))}
      </button>
      <button
        className="btn"
        style={{ fontSize: 12 }}
        onClick={() => { resetBinding(id); setConflict(null); setTick((n) => n + 1); onChanged() }}
      >
        {t('settings.resetKey')}
      </button>
    </div>
  )
}

/** 模板编辑表单（global-config 票 02）：内置行「编辑」= 同名覆盖层创建
 *  （改名即另存副本）；自定义行可直接改可删。校验交给后端 fail-closed。 */
function TemplateEditor({ def, names, isCustom, onDone }: {
  def: RoleDef
  names: string[]
  isCustom: boolean
  onDone: () => void
}) {
  const { t } = useTranslation()
  const { pushToast, askConfirm } = useUiStore()
  const [name, setName] = useState(def.name)
  const [duty, setDuty] = useState(def.duty)
  const [reviewer, setReviewer] = useState(def.reviewer ?? '')
  const [slot, setSlot] = useState(def.model_slot)
  const [globs, setGlobs] = useState(def.globs.join('\n'))
  const [skills, setSkills] = useState(def.skills.join(', '))
  const [busy, setBusy] = useState(false)
  const [pv, setPv] = useState<ProvidersView | null>(null)

  useEffect(() => { api.listProviders().then(setPv).catch(() => {}) }, [])

  const input: React.CSSProperties = {
    width: '100%', padding: '7px 10px', fontSize: 13,
    // ui-polish-2 ③ 凹槽井：与 .input 原语同款（--bg 底+border-strong），面板内不再隐形
    background: 'var(--bg)', border: '1px solid var(--border-strong)', borderRadius: 6,
    color: 'var(--text)', fontFamily: 'inherit',
  }
  const label: React.CSSProperties = { fontSize: 12, fontWeight: 560, color: 'var(--text-3)', marginTop: 10 }

  const save = async () => {
    setBusy(true)
    try {
      await api.saveRoleTemplate({
        name: name.trim(), duty: duty.trim(),
        reviewer: reviewer || null, model_slot: slot.trim() || 'default',
        globs: globs.split('\n').map((s) => s.trim()).filter(Boolean),
        skills: skills.split(',').map((s) => s.trim()).filter(Boolean),
      })
      onDone()
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  const del = () =>
    askConfirm({
      title: t('teamTpl.confirmDel', { name: def.name }),
      danger: true,
      run: async () => {
        setBusy(true)
        try {
          await api.deleteRoleTemplate(def.name)
          onDone()
        } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
      },
    })

  return (
    <div className="panel" style={{ padding: '12px 14px' }}>
      <div style={label}>{t('agent.roleName')}</div>
      <input value={name} onChange={(e) => setName(e.target.value)} style={input} />
      {!isCustom && <div className="dim3" style={{ fontSize: 10, marginTop: 3 }}>{t('teamTpl.builtinHint')}</div>}
      <div style={label}>{t('agent.duty')}</div>
      <textarea value={duty} onChange={(e) => setDuty(e.target.value)} rows={2} style={{ ...input, resize: 'vertical' }} />
      <div style={{ display: 'flex', gap: 8 }}>
        <div style={{ flex: 1 }}>
          <div style={label}>{t('agent.reviewer')}</div>
          <select value={reviewer} onChange={(e) => setReviewer(e.target.value)} style={input}>
            <option value="">{t('agent.noReviewer')}</option>
            {names.filter((n) => n !== def.name).map((n) => <option key={n} value={n}>{n}</option>)}
          </select>
        </div>
        <div style={{ flex: 1 }}>
          <div style={label}>{t('agent.modelSlot')}</div>
          <select value={slot} onChange={(e) => setSlot(e.target.value)} style={input}>
            {sharedSlots(pv?.slots ?? {}, slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, pv, t('agent.dedicatedTag'))}</option>
            ))}
          </select>
        </div>
      </div>
      <div style={label}>{t('agent.globs')}</div>
      <textarea value={globs} onChange={(e) => setGlobs(e.target.value)} rows={2}
        placeholder="src/**" style={{ ...input, fontFamily: 'monospace', resize: 'vertical' }} />
      <div style={label}>{t('agent.skills')}</div>
      <EntityChips value={skills.split(',').map((s) => s.trim()).filter(Boolean)} onChange={(ids) => setSkills(ids.join(', '))} source="skills" />
      <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
        <button className="btn primary" disabled={busy || !name.trim()} onClick={save}>{t('teamTpl.saveTpl')}</button>
        {isCustom && <button className="btn" disabled={busy} onClick={del}>{t('teamTpl.delTpl')}</button>}
        <button className="btn" onClick={onDone}>{t('agent.cancel')}</button>
      </div>
    </div>
  )
}

/** 角色模板库（ADR 0057 + settings-3col 票 03）：内置∪~/.hexagon/roles.json，
 *  中列清单|右列 TemplateEditor。无项目也能编辑——模板是可复用配置不是
 *  项目状态；建项目时按名复制物化，不回写。 */
function TemplateLibrary() {
  const { t } = useTranslation()
  const [tpls, setTpls] = useState<RoleTemplate[] | null>(null)
  const [sel, setSel] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)

  const load = useCallback(() => {
    api.listRoleTemplates().then(setTpls).catch(() => setTpls([]))
  }, [])
  useEffect(load, [load])

  const names = (tpls ?? []).map((x) => x.def.name)
  const blank: RoleDef = { name: '', duty: '', reviewer: null, model_slot: 'chat', globs: [], skills: [] }
  const selTpl = tpls?.find((x) => x.def.name === sel) ?? null

  return (
    <div>
      <div className="dim3" style={{ fontSize: 12, margin: '0 0 8px' }}>{t('teamTpl.library')}</div>
      <ListDetail<RoleTemplate>
        items={tpls ?? []}
        itemKey={(x) => x.def.name}
        filterText={(x) => `${x.def.name} ${x.def.duty}`}
        selected={creating ? null : sel}
        onSelect={(k) => { setSel(k); setCreating(false) }}
        searchPlaceholder={t('teamTpl.filter')}
        emptyText={t('teamTpl.empty')}
        actions={
          <button className="btn" style={{ fontSize: 12 }} onClick={() => { setCreating(true); setSel(null) }}>
            + {t('teamTpl.newTpl')}
          </button>
        }
        renderItem={(tp) => (
          <>
            <span style={{ flex: 1, minWidth: 0, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {tp.def.name}
            </span>
            <span className={`chip ${tp.origin === 'custom' ? 'ok' : ''}`} style={{ fontSize: 9 }}>
              {t(`teamTpl.${tp.origin}`)}
            </span>
          </>
        )}
      >
        {creating ? (
          <TemplateEditor
            def={blank} names={names} isCustom
            onDone={() => { setCreating(false); load() }}
          />
        ) : selTpl ? (
          <TemplateEditor
            key={selTpl.def.name}
            def={selTpl.def} names={names} isCustom={selTpl.origin === 'custom'}
            onDone={() => { setSel(null); load() }}
          />
        ) : (
          <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center' }}>
            {t('teamTpl.unselected')}
          </div>
        )}
      </ListDetail>
    </div>
  )
}

/** 团队分区（ui-audit-2 票 02 / global-config 票 02 / settings-3col 票 03）：
 *  项目内 = 中列 Agent 清单 | 右列 RoleEditor；「+ 模板添加」在右列出
 *  模板选择面板（选中即 createRole(def) 复制物化进项目）——模板库本体
 *  不再平铺（选材≠并列对象）。projectless = 仅模板库。 */
function TeamSection({ projectless = false }: { projectless?: boolean }) {
  const { t } = useTranslation()
  const { team, providers, refreshSlow, pushToast } = useUiStore()
  const [sel, setSel] = useState<string | null>(null)
  const [addingTpl, setAddingTpl] = useState(false)
  const [creating, setCreating] = useState(false)
  const [tpls, setTpls] = useState<RoleTemplate[] | null>(null)
  const [editTpl, setEditTpl] = useState<RoleTemplate | null>(null)

  // Launcher 进设置时 team 可能是空的——分区挂载即补拉慢切片。
  // projectless 无项目态：不发项目 IPC（team 拉取必败），只渲染模板库。
  useEffect(() => {
    if (!projectless) void refreshSlow(['team'])
  }, [refreshSlow, projectless])

  useEffect(() => {
    if (addingTpl && tpls === null) {
      api.listRoleTemplates().then(setTpls).catch(() => setTpls([]))
    }
  }, [addingTpl, tpls])

  if (projectless) return <TemplateLibrary />

  const selAgent = team.find((m) => m.id === sel) ?? null

  const addFromTpl = async (tp: RoleTemplate) => {
    try {
      await api.createRole(tp.def)
      pushToast(t('teamTpl.added', { name: tp.def.name }), 'ok')
      setAddingTpl(false)
      await refreshSlow(['team'])
    } catch (e) { pushToast(errText(e), 'err') }
  }

  return (
    <ListDetail
      items={team}
      itemKey={(m) => m.id}
      filterText={(m) => m.role}
      selected={sel}
      onSelect={(k) => { setSel(k); setAddingTpl(false); setCreating(false); setEditTpl(null) }}
      searchPlaceholder={t('teamTpl.filterAgents')}
      emptyText={t('side.noMembers')}
      actions={
        <>
          <button className="btn" style={{ fontSize: 12 }} onClick={() => { setAddingTpl(true); setCreating(false); setSel(null); setEditTpl(null) }}>
            + {t('teamTpl.fromTpl')}
          </button>
          <button className="btn" style={{ fontSize: 12 }} onClick={() => { setCreating(true); setAddingTpl(false); setSel(null); setEditTpl(null) }}>
            + {t('agent.createRole')}
          </button>
        </>
      }
      renderItem={(m) => (
        <>
          <Avatar agentId={m.id} role={m.role} size={20} />
          <span className={`dot ${m.status === 'active' ? 'on' : 'off'}`} />
          <span style={{ flex: 1, minWidth: 0, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {m.role}
          </span>
          <span className="dim3 mono" style={{ fontSize: 9 }}>
            {slotLabel(m.model_slot, providers, t('agent.dedicatedTag'))}
          </span>
        </>
      )}
    >
      {creating ? (
        <CreateRoleForm density="md" onDone={() => { setCreating(false); void refreshSlow(['team']) }} />
      ) : editTpl ? (
        <TemplateEditor
          key={editTpl.def.name}
          def={editTpl.def}
          names={(tpls ?? []).map((x) => x.def.name)}
          isCustom={editTpl.origin === 'custom'}
          onDone={() => { setEditTpl(null); api.listRoleTemplates().then(setTpls).catch(() => {}) }}
        />
      ) : addingTpl ? (
        <div className="panel" style={{ padding: '10px 12px' }}>
          <div className="dim3" style={{ fontSize: 12, marginBottom: 8 }}>{t('teamTpl.pickTpl')}</div>
          {(tpls ?? []).map((tp) => (
            <div key={tp.def.name} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '6px 0', borderBottom: '1px solid var(--border)', fontSize: 12 }}>
              <span style={{ fontWeight: 510 }}>{tp.def.name}</span>
              <span className={`chip ${tp.origin === 'custom' ? 'ok' : ''}`} style={{ fontSize: 9 }}>{t(`teamTpl.${tp.origin}`)}</span>
              <span className="dim3" style={{ flex: 1, minWidth: 0, fontSize: 10, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{tp.def.duty}</span>
              <button className="btn" style={{ fontSize: 12 }} onClick={() => setEditTpl(tp)}>{t('mcp.edit')}</button>
              <button className="btn primary" style={{ fontSize: 12 }} onClick={() => void addFromTpl(tp)}>{t('teamTpl.add')}</button>
            </div>
          ))}
          {tpls !== null && tpls.length === 0 && (
            <div className="dim3" style={{ fontSize: 12, padding: '8px 0' }}>{t('teamTpl.empty')}</div>
          )}
        </div>
      ) : selAgent ? (
        <RoleEditor key={selAgent.id} agentId={selAgent.id} density="md" onClose={() => setSel(null)} />
      ) : (
        <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center' }}>
          {t('teamTpl.unselectedAgent')}
        </div>
      )}
    </ListDetail>
  )
}

/** 权限分区（ui-audit-2 票 03 / report A2+C）：「记住」的规则此前持久化后
 *  无处查看、无法撤销——单行道是安全缺口。这里列全表 + 撤销走确认层。
 *  撤销成功回落的语义：同形状请求重新弹待决卡（后端测试钉了）。 */
function PermsSection() {
  const { t } = useTranslation()
  const { pushToast, askConfirm } = useUiStore()
  const [rules, setRules] = useState<PermissionRuleRow[] | null>(null)

  const load = useCallback(
    () => api.permissionRules().then(setRules).catch((e) => pushToast(errText(e), 'err')),
    [pushToast],
  )
  useEffect(() => { void load() }, [load])

  const revoke = (r: PermissionRuleRow) =>
    askConfirm({
      title: t('perms.revokeTitle'),
      body: `${r.tool} · ${r.shape}`,
      danger: true,
      confirmLabel: t('perms.revoke'),
      run: async () => {
        await api.revokePermissionRule(r.id).catch((e) => pushToast(errText(e), 'err'))
        await load()
      },
    })

  return (
    <div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10 }}>{t('perms.intro')}</div>
      {rules === null ? (
        <div className="dim3" style={{ fontSize: 12, padding: '12px 0' }}>…</div>
      ) : rules.length === 0 ? (
        <div className="dim3" style={{ fontSize: 12, padding: '12px 0' }}>{t('perms.empty')}</div>
      ) : (
        rules.map((r) => (
          <div
            key={r.id}
            style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--border)', fontSize: 12 }}
          >
            <span className="chip">{r.tool}</span>
            {r.effect === 'allow' && <span className="chip">{t(r.project_shared ? 'projectPermission.sharedLabel' : 'projectPermission.legacy')}</span>}
            {r.agent_id && <span className="chip">{r.agent_id}</span>}
            <div style={{ flex: 1, minWidth: 0 }}>
              {r.project_shared ? <PermissionScope tool={r.tool} shape={r.shape} generalized={!r.shape.startsWith('exact:')} /> : <div className="mono" title={r.shape}>{r.shape}</div>}
              {r.tool === 'bash' && <div className="dim3">
                {t(r.network_allowed ? 'perms.networkAllowed' : 'perms.networkDisabled')} · {t(r.background_allowed ? 'perms.backgroundAllowed' : 'perms.foregroundOnly')} · {r.session_name ? t('perms.namedSession', { name: r.session_name }) : t('perms.noNamedSession')}
              </div>}
            </div>
            <span className={`chip ${r.effect === 'deny' ? 'err' : 'ok'}`} style={{ fontSize: 10 }}>
              {t(`perms.effect_${r.effect}`)}
            </span>
            <span className="chip" style={{ fontSize: 10 }}>{t(`perms.scope_${r.scope}`)}</span>
            <span className="dim3" style={{ fontSize: 10 }}>{r.created_at.slice(0, 10)}</span>
            <button className="btn" style={{ fontSize: 12 }} onClick={() => revoke(r)}>
              {t('perms.revoke')}
            </button>
          </div>
        ))
      )}
      <div className="dim3" style={{ fontSize: 12, marginTop: 10 }}>{t('perms.grantsHint')}</div>
    </div>
  )
}

/** 技能详情（global-config 票 03，参考 Cherry Studio 三栏）：文件树 +
 *  SKILL.md 渲染；全局技能可「编辑」成表单（名称/描述/正文 →
 *  save_global_skill 落 ~/.hexagon/skills/<name>/）。内置/项目只读。 */
function SkillDetail({ skill, onSaved }: { skill: SkillRow; onSaved: () => void }) {
  const { t } = useTranslation()
  const { pushToast } = useUiStore()
  const [files, setFiles] = useState<string[]>([])
  const [sel, setSel] = useState('SKILL.md')
  const [content, setContent] = useState('')
  const [readError, setReadError] = useState('')
  const [reading, setReading] = useState(true)
  const [readRevision, setReadRevision] = useState(0)
  const [projectDocument, setProjectDocument] = useState<ProjectSkillDocument | null>(null)
  const [editing, setEditing] = useState(false)
  const [desc, setDesc] = useState(skill.description)
  const [body, setBody] = useState('')
  // origBody 只装 SKILL.md 原文——content 跟的是当前选中文件（可能不是
  // SKILL.md），取消编辑时拿它恢复会把别文件的正文灌进编辑框。
  const [origBody, setOrigBody] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let current = true
    api.skillFiles(skill.name).then(value => { if (current) setFiles(value) }).catch(() => { if (current) setFiles(['SKILL.md']) })
    return () => { current = false }
  }, [skill.name])

  useEffect(() => {
    // Governance 01: an unreadable original is not an empty successful read;
    // a delayed response for a previous file must not replace the current one.
    let current = true
    setReadError('')
    setReading(true)
    setContent('')
    api.readSkillFile(skill.name, sel)
      .then((c) => {
        if (!current) return
        setContent(c)
        if (sel === 'SKILL.md') { setBody(c); setOrigBody(c) }
      })
      .catch((e) => { if (current) setReadError(errText(e)) })
      .finally(() => { if (current) setReading(false) })
    return () => { current = false }
  }, [skill.name, sel, readRevision])

  const toggleEdit = async () => {
    if (editing) { setEditing(false); return }
    if (skill.origin === 'project') {
      try {
        const document = await api.projectSkillDocument(skill.name)
        setProjectDocument(document)
        setBody(document.content)
        setOrigBody(document.content)
      } catch (e) { pushToast(errText(e), 'err'); return }
    }
    setEditing(true)
  }

  const save = async () => {
    setBusy(true)
    try {
      // body 是整份 SKILL.md 原文——直接落盘（不重组 frontmatter，
      // 用户可见的就是写的）。
      const m = body.match(/^---\n([\s\S]*?)\n---\n?([\s\S]*)$/)
      const fmName = m?.[1].match(/^name:\s*(.+)$/m)?.[1]?.trim() ?? skill.name
      const fmDesc = m?.[1].match(/^description:\s*(.+)$/m)?.[1]?.trim() ?? desc
      // Governance 08: project edits never route through the global writer.
      // The host verifies both project root and the version read on editor entry.
      let projectFresh: ProjectSkillDocument | null = null
      if (skill.origin === 'project') {
        if (!projectDocument || projectDocument.skill !== skill.name) throw new Error(t('experience.changed'))
        projectFresh = await api.saveProjectSkillDocument({ ...projectDocument, content: body })
        setProjectDocument(projectFresh)
      } else {
        await api.saveGlobalSkill(fmName, fmDesc, m?.[2] ?? body)
      }
      // 落盘后回读 SKILL.md：服务端按 frontmatter 规范化重写文件，body 与盘上
      // 文本不等价。不同步的话视图停在保存前，origBody 快照也过期——下次取消
      // 会把 pre-save 文本灌回编辑框，再保存即静默回滚。
      const fresh = projectFresh?.content ?? await api.readSkillFile(skill.name, 'SKILL.md')
      if (sel === 'SKILL.md') setContent(fresh)
      setBody(fresh)
      setOrigBody(fresh)
      setDesc(fmDesc)
      setEditing(false)
      setReadRevision((n) => n + 1)
      onSaved()
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  // 取消编辑：字段回到最后加载的 SKILL.md 原文，放弃未保存修改。
  const cancelEdit = () => {
    setDesc(skill.description)
    setBody(origBody)
    setEditing(false)
  }

  const isMd = sel.endsWith('.md') || sel === 'SKILL.md'
  // ui-polish-3：右列详情统一 .panel 圆角边线卡。
  // 面板限高 72vh（与左列同；负责人反馈 2026-09：详情全屏滚动、编辑区过矮）——
  // 头行与文件签钉住，滚动只发生在内容区，不把整个设置列顶出去；
  // 编辑态 textarea flex 撑高取代原 rows=14 矮框。
  return (
    <div className="panel" onKeyDown={(e) => {
      if (skill.origin !== 'project' || e.repeat || busy) return
      if (matches(e.nativeEvent, bindingFor('editProjectSkill'))) { e.preventDefault(); e.stopPropagation(); void toggleEdit() }
      else if (editing && matches(e.nativeEvent, bindingFor('saveProjectSkill'))) { e.preventDefault(); e.stopPropagation(); void save() }
    }} style={{ padding: '12px 14px', maxHeight: '72vh', display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8, flexShrink: 0 }}>
        <strong style={{ fontSize: 13 }}>{skill.name}</strong>
        <span className={`chip ${skill.origin === 'builtin' ? '' : 'ok'}`} style={{ fontSize: 10 }}>
          {t(`skills.origin_${skill.origin}`)}
        </span>
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
          {(skill.origin === 'global' || skill.origin === 'project') && (
            <button className="btn" style={{ fontSize: 12 }} onClick={() => void toggleEdit()} title={skill.origin === 'project' ? `${t('skills.edit')} (${formatBinding(bindingFor('editProjectSkill'))})` : undefined}>
              {editing ? t('skills.view') : t('skills.edit')}
            </button>
          )}
        </span>
      </div>
      {readError && <p role="alert">{readError}</p>}
      {reading && <LoadingState label={t('skills.loadingDetail')} />}
      {skill.origin === 'project' && <ExperienceEntries key={skill.name} skill={skill.name} documentRevision={readRevision} onRecovered={() => { setReadRevision((n) => n + 1); onSaved() }} />}
      {!!skill.legacy_experience_blocks && (
        <p role="status">{t('skills.legacyExperience', { count: skill.legacy_experience_blocks })}</p>
      )}
      {files.length > 1 && (
        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginBottom: 8, flexShrink: 0 }}>
          {files.map((f) => (
            <button
              key={f}
              className={`btn mono ${sel === f ? 'primary' : ''}`}
              style={{ fontSize: 12, padding: '3px 10px' }}
              onClick={() => setSel(f)}
            >
              {f}
            </button>
          ))}
        </div>
      )}
      {editing ? (
        <div style={{ flex: 1, minHeight: 0, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
          <div style={{ fontSize: 12, fontWeight: 560, color: 'var(--text-3)', marginBottom: 4, flexShrink: 0 }}>
            {t('skills.descLabel')}
          </div>
          <input
            value={desc}
            disabled={skill.origin === 'project'}
            onChange={(e) => setDesc(e.target.value)}
            className="input" style={{ fontFamily: 'inherit', flexShrink: 0 }}
          />
          <div style={{ fontSize: 12, fontWeight: 560, color: 'var(--text-3)', margin: '8px 0 4px', flexShrink: 0 }}>
            SKILL.md
          </div>
          <textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            className="input"
            style={{ fontFamily: 'monospace', resize: 'none', flex: 1, minHeight: '40vh' }}
          />
          <div style={{ display: 'flex', gap: 8, marginTop: 8, flexShrink: 0 }}>
            <button className="btn primary" style={{ fontSize: 12 }} disabled={busy} onClick={save} title={skill.origin === 'project' ? `${t('skills.save')} (${formatBinding(bindingFor('saveProjectSkill'))})` : undefined}>
              {t('skills.save')}
            </button>
            <button className="btn" style={{ fontSize: 12 }} disabled={busy} onClick={cancelEdit}>
              {t('skills.cancel')}
            </button>
          </div>
        </div>
      ) : (
        <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
          {isMd ? (
            <div className="panel" style={{ padding: '10px 14px', fontSize: 12 }}>
              <Md>{content}</Md>
            </div>
          ) : (
            <CodeBlock code={content} />
          )}
        </div>
      )}
    </div>
  )
}

/** 技能分区（ui-audit-2 票 04 → global-config 票 03 双栏重做）：
 *  左=已装列表（计数+筛选+启停+来源徽标），右=详情/空态安装入口。
 *  projectless 时 list_skills 回落全局层——同一面直接可用。 */
function SkillsSection() {
  const { t } = useTranslation()
  const { pushToast } = useUiStore()
  const [skills, setSkills] = useState<SkillRow[] | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [newName, setNewName] = useState('')
  const [newDesc, setNewDesc] = useState('')
  const [busy, setBusy] = useState(false)
  // 票 04：本机外部技能扫描导入
  const [scanRows, setScanRows] = useState<ExtSkillRow[] | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [scanning, setScanning] = useState(false)

  const loadGeneration = useRef(0)
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const load = useCallback(async () => {
    const generation = ++loadGeneration.current
    setLoading(true)
    setLoadError('')
    try {
      const rows = await api.listSkills()
      if (generation === loadGeneration.current) setSkills(rows)
    } catch (error) {
      if (generation === loadGeneration.current) setLoadError(errText(error))
    } finally {
      if (generation === loadGeneration.current) setLoading(false)
    }
  }, [])
  useEffect(() => {
    void load()
    return () => { loadGeneration.current += 1 }
  }, [load])

  const toggle = (s: SkillRow) => {
    const next = !s.enabled
    setSkills((prev) => prev?.map((x) => (x.name === s.name ? { ...x, enabled: next } : x)) ?? prev)
    api.setSkillMuted(s.name, next).catch((e) => {
      pushToast(errText(e), 'err')
      void load()
    })
  }

  const create = async () => {
    if (!newName.trim()) return
    setBusy(true)
    try {
      await api.saveGlobalSkill(newName.trim(), newDesc.trim(), `# ${newName.trim()}\n`)
      setCreating(false); setNewName(''); setNewDesc('')
      await load()
      setSelected(newName.trim())
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  const scan = async () => {
    setScanning(true)
    try {
      const rows = await api.scanExternalSkills()
      setScanRows(rows)
      // 默认可导入的全勾（conflict 行禁选）
      setPicked(new Set(rows.filter((r) => !r.conflict).map((r) => r.path)))
    } catch (e) { pushToast(errText(e), 'err') } finally { setScanning(false) }
  }

  const doImport = async () => {
    setBusy(true)
    try {
      const rep = await api.importSkills([...picked])
      pushToast(t('skills.importResult', { n: rep.imported, m: rep.skipped.length }), 'ok')
      setScanRows(null)
      await load()
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  const installFrom = async (directory: boolean) => {
    if (!isTauri) return
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const pickedPath = await open(directory
        ? { directory: true }
        : { filters: [{ name: 'ZIP', extensions: ['zip'] }] })
      if (typeof pickedPath !== 'string') return
      const name = await api.installSkillPath(pickedPath)
      pushToast(t('skills.installedOk', { name }), 'ok')
      await load()
      setSelected(name)
    } catch (e) { pushToast(errText(e), 'err') }
  }

  const selSkill = skills?.find((s) => s.name === selected) ?? null

  return (
    <div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10 }}>{t('skills.intro')}</div>
      {loading && <LoadingState label={t('skills.loadingList')} />}
      {loadError && <div role="alert">{loadError} <button className="btn" onClick={() => void load()}
        title={[t('skills.retryLoad'), formatBinding(bindingFor('retrySkillsLoad'))].filter(Boolean).join(' · ')}
        onKeyDown={event => { if (matches(event.nativeEvent, bindingFor('retrySkillsLoad'))) { event.preventDefault(); event.stopPropagation(); void load() } }}>{t('skills.retryLoad')}</button></div>}
      <ListDetail<SkillRow>
        virtualized
        items={skills ?? []}
        itemKey={(s) => s.name}
        filterText={(s) => `${s.name} ${s.description}`}
        selected={creating ? null : selected}
        onSelect={(k) => { setSelected(k); setCreating(false) }}
        searchPlaceholder={t('skills.filter')}
        emptyText={loading ? t('skills.loadingList') : loadError ? '—' : t('skills.empty')}
        actions={
          <>
            <button className="btn" style={{ fontSize: 12 }} onClick={() => { setCreating(true); setSelected(null) }}>
              + {t('skills.new')}
            </button>
            <button className="btn" style={{ fontSize: 12 }} disabled={scanning} onClick={scan}>
              {scanning ? t('skills.scanning') : t('skills.scan')}
            </button>
            {isTauri && (
              <>
                <button className="btn" style={{ fontSize: 12 }} onClick={() => installFrom(true)}>
                  {t('skills.installDir')}
                </button>
                <button className="btn" style={{ fontSize: 12 }} onClick={() => installFrom(false)}>
                  {t('skills.installZip')}
                </button>
              </>
            )}
          </>
        }
        renderItem={(s) => (
          <>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontWeight: 510, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{s.name}</div>
              <div className="dim3" style={{ fontSize: 10, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{s.description}</div>
            </div>
            <span className="chip" style={{ fontSize: 9, flexShrink: 0 }}>{t(`skills.origin_${s.origin}`)}</span>
            <input
              type="checkbox"
              className="switch"
              checked={s.enabled}
              title={t('skills.enabled')}
              onClick={(e) => e.stopPropagation()}
              onChange={() => toggle(s)}
            />
          </>
        )}
      >
        {creating ? (
          <div className="panel" style={{ padding: '12px 14px' }}>
            <div style={{ fontSize: 12, fontWeight: 560, color: 'var(--text-3)', marginBottom: 4 }}>{t('skills.nameLabel')}</div>
            <input className="input" value={newName} onChange={(e) => setNewName(e.target.value)}
              placeholder="my-skill" style={{ fontFamily: 'monospace' }} />
            <div style={{ fontSize: 12, fontWeight: 560, color: 'var(--text-3)', margin: '8px 0 4px' }}>{t('skills.descLabel')}</div>
            <input className="input" value={newDesc} onChange={(e) => setNewDesc(e.target.value)} />
            <button className="btn primary" style={{ marginTop: 10, fontSize: 12 }} disabled={busy || !newName.trim()} onClick={create}>
              {t('skills.create')}
            </button>
            <div className="dim3" style={{ fontSize: 10, marginTop: 8 }}>{t('skills.createHint')}</div>
          </div>
        ) : selSkill ? (
          <SkillDetail key={selSkill.name} skill={selSkill} onSaved={load} />
        ) : (
          <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center', lineHeight: 1.9 }}>
            {t('skills.unselected')}
          </div>
        )}
      </ListDetail>
      {/* 票 04：外部技能扫描结果——去重列表，冲突项禁选（导入不覆盖本机全局同名）。
          面板限高 40vh（负责人反馈 2026-09：扫描结果平铺把整页顶出去）——
          标题与底部操作钉住，只有行列表滚。 */}
      {scanRows !== null && (
        <div className="panel" style={{ marginTop: 12, padding: '10px 12px', maxHeight: '40vh', display: 'flex', flexDirection: 'column' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6, flexShrink: 0 }}>
            <strong style={{ fontSize: 12 }}>{t('skills.scanTitle')}</strong>
            <span className="dim3" style={{ fontSize: 10 }}>{t('skills.scanHint')}</span>
          </div>
          {scanRows.length === 0 ? (
            <div className="dim3" style={{ fontSize: 12, padding: '8px 0' }}>{t('skills.scanEmpty')}</div>
          ) : (
            <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
              {scanRows.map((r) => (
              <label
                key={r.path}
                style={{
                  display: 'flex', alignItems: 'center', gap: 8, padding: '5px 0',
                  fontSize: 12, opacity: r.conflict ? 0.5 : 1,
                  cursor: r.conflict ? 'default' : 'pointer',
                }}
              >
                <input
                  type="checkbox"
                  disabled={r.conflict}
                  checked={picked.has(r.path)}
                  onChange={(e) => {
                    const n = new Set(picked)
                    if (e.target.checked) n.add(r.path); else n.delete(r.path)
                    setPicked(n)
                  }}
                />
                <span style={{ fontWeight: 510 }}>{r.name}</span>
                <span className="chip" style={{ fontSize: 9 }}>{r.origin}</span>
                {r.conflict && <span className="chip warn" style={{ fontSize: 9 }}>{t('skills.conflict')}</span>}
                <span className="dim3" style={{ fontSize: 10, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{r.description}</span>
              </label>
              ))}
            </div>
          )}
          <div style={{ display: 'flex', gap: 8, marginTop: 8, flexShrink: 0 }}>
            <button
              className="btn"
              style={{ fontSize: 12 }}
              onClick={() => setPicked(new Set(scanRows.filter((r) => !r.conflict).map((r) => r.path)))}
            >
              {t('skills.pickAll')}
            </button>
            <button className="btn primary" style={{ fontSize: 12 }} disabled={busy || picked.size === 0} onClick={doImport}>
              {t('skills.importN', { n: picked.size })}
            </button>
            <button className="btn" style={{ fontSize: 12 }} onClick={() => setScanRows(null)}>
              {t('agent.cancel')}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

/** MCP 分区（global-config 票 05）：全局清单 ~/.hexagon/mcp.json ∪ 项目
 *  .hexagon/mcp.json（同名项目覆盖），无项目可增删改全局层——配置清单≠授权
 *  （ADR 0010，授权仍按 Agent 走团队区 grants）。远程（url）条目可见但一期
 *  不 spawn。实况块只在有项目时渲染（宿主生命周期是项目级的）。 */
function McpSection({ projectless }: { projectless: boolean }) {
  const { t } = useTranslation()
  const { pushToast } = useUiStore()
  const [rows, setRows] = useState<McpServiceRow[] | null>(null)
  const [entries, setEntries] = useState<McpEntryRow[] | null>(null)
  const [sel, setSel] = useState<string | null>(null) // `${origin}:${name}`
  const [creating, setCreating] = useState(false)
  const [busy, setBusy] = useState(false)
  // 票 06：本机 MCP 扫描导入
  const [scanRows, setScanRows] = useState<ExtMcpRow[] | null>(null)
  const [picked, setPicked] = useState<Set<number>>(new Set())
  const [scanning, setScanning] = useState(false)

  const load = useCallback(async () => {
    try {
      setEntries(await api.mcpEntries())
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }, [pushToast])

  useEffect(() => {
    api.mcpEntries().then(setEntries).catch((e) => pushToast(errText(e), 'err'))
    if (!projectless) {
      api.mcpServices().then(setRows).catch((e) => pushToast(errText(e), 'err'))
    }
  }, [projectless, pushToast])

  const del = async (name: string) => {
    setBusy(true)
    try {
      await api.deleteMcpService(name)
      await load()
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  const scan = async () => {
    setScanning(true)
    try {
      const rows = await api.scanExternalMcp()
      setScanRows(rows)
      setPicked(new Set(rows.map((_, i) => i).filter((i) => !rows[i].conflict)))
    } catch (e) { pushToast(errText(e), 'err') } finally { setScanning(false) }
  }

  const doImport = async () => {
    if (!scanRows) return
    setBusy(true)
    try {
      const references = [...picked].map((i) => scanRows[i].reference)
      const rep = await api.importMcp(references)
      pushToast(t('mcp.importResult', { n: rep.imported, m: rep.skipped.length }), 'ok')
      setScanRows(null)
      await load()
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  const openMarket = async () => {
    try {
      await api.openMcpMarket()
    } catch (e) { pushToast(errText(e), 'err') }
  }

  // 行内启停：仅全局 stdio 行可切（project 层文件手改；remote 一期不 spawn）
  const toggle = async (s: McpEntryRow) => {
    try {
      await api.saveMcpService({
        name: s.name, command: s.command, args: s.args, env: s.env,
        cwd: s.cwd, disabled: !s.disabled, url: s.url, headers: s.headers,
      })
      await load()
    } catch (e) { pushToast(errText(e), 'err') }
  }

  const selEntry = entries?.find((e) => `${e.origin}:${e.name}` === sel) ?? null
  const selLive = selEntry ? rows?.find((r) => r.name === selEntry.name) : undefined

  return (
    <div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10, lineHeight: 1.7 }}>
        {t('mcp.intro')}<p>{t('dataBoundary.edit')}</p><p>{t('dataBoundary.mcpStorage')}</p>
      </div>
      <ListDetail<McpEntryRow>
        items={entries ?? []}
        itemKey={(e) => `${e.origin}:${e.name}`}
        filterText={(e) => `${e.name} ${e.command} ${e.url ?? ''}`}
        selected={creating ? null : sel}
        onSelect={(k) => { setSel(k); setCreating(false) }}
        searchPlaceholder={t('mcp.filter')}
        emptyText={t('mcp.empty')}
        actions={
          <>
            <button className="btn" style={{ fontSize: 12 }} onClick={() => { setCreating(true); setSel(null) }}>
              + {t('mcp.new')}
            </button>
            <button className="btn" style={{ fontSize: 12 }} disabled={scanning} onClick={scan}>
              {scanning ? t('mcp.scanning') : t('mcp.scan')}
            </button>
            {isTauri && (
              <button className="btn" style={{ fontSize: 12 }} onClick={openMarket}>
                {t('mcp.market')}
              </button>
            )}
          </>
        }
        renderItem={(s) => (
          <>
            <span style={{ flex: 1, minWidth: 0, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {s.name}
            </span>
            <span className="chip" style={{ fontSize: 9 }}>
              {s.origin === 'global' ? t('mcp.origin_global') : t('mcp.origin_project')}
            </span>
            {s.transport === 'remote' && (
              <span className="chip warn" style={{ fontSize: 9 }}>{t('mcp.remote')}</span>
            )}
            {s.origin === 'global' && s.transport === 'stdio' ? (
              <input
                type="checkbox"
                className="switch"
                checked={!s.disabled}
                title={t('mcp.enabled')}
                onClick={(e) => e.stopPropagation()}
                onChange={() => void toggle(s)}
              />
            ) : (
              s.disabled && <span className="chip" style={{ fontSize: 9 }}>{t('mcp.disabledTag')}</span>
            )}
          </>
        )}
      >
        {creating ? (
          <McpServiceForm
            entry={null}
            onDone={async () => { setCreating(false); await load() }}
            onCancel={() => setCreating(false)}
          />
        ) : selEntry ? (
          <>
            {/* key=sel：表单字段是 useState(entry…) 首挂载初始化——不挂 key 换行不换内容（ui-polish-2 ②） */}
            <McpServiceForm
              key={sel}
              entry={selEntry}
              readOnly={selEntry.origin !== 'global'}
              onDone={async () => { setSel(null); await load() }}
              onCancel={() => setSel(null)}
              onDelete={selEntry.origin === 'global' ? async () => { await del(selEntry.name); setSel(null) } : undefined}
            />
            {!projectless && selLive && (
              <div className="panel" style={{ marginTop: 10, padding: '10px 12px' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span className={`dot ${selLive.status === 'up' ? 'on' : 'off'}`} />
                  <span style={{ fontSize: 12, fontWeight: 510 }}>{t('mcp.live')}</span>
                  <span className={`chip ${selLive.status === 'up' ? 'ok' : selLive.status === 'starting' ? 'warn' : 'err'}`} style={{ fontSize: 10 }}>
                    {selLive.status === 'up' ? t('mcp.up', { count: selLive.tools.length }) : selLive.status === 'starting' ? t('mcp.starting') : t('mcp.down')}
                  </span>
                </div>
                {selLive.status === 'up' && selLive.tools.length > 0 && (
                  <div className="dim3 mono" style={{ fontSize: 10, marginTop: 6 }}>
                    {selLive.tools.map((n) => `mcp:${selLive.name}:${n}`).join('  ')}
                  </div>
                )}
                {selLive.error && (
                  <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 6 }}>{selLive.error}</div>
                )}
              </div>
            )}
          </>
        ) : (
          <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center' }}>
            {t('mcp.unselected')}
          </div>
        )}
      </ListDetail>
      {/* 票 06：外部 MCP 扫描结果——去重列表；冲突禁选（不覆盖本机全局），
          远程传输可导入但落 disabled 态（一期不 spawn） */}
      {/* 同 skills 扫描面板：限高 40vh，标题/操作钉住，只有行列表滚
          （负责人反馈 2026-09：扫描结果平铺把整页顶出去）。 */}
      {scanRows !== null && (
        <div className="panel" style={{ marginTop: 8, padding: '10px 12px', maxHeight: '40vh', display: 'flex', flexDirection: 'column' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6, flexShrink: 0 }}>
            <strong style={{ fontSize: 12 }}>{t('mcp.scanTitle')}</strong>
            <span className="dim3" style={{ fontSize: 10 }}>{t('mcp.scanHint')}</span>
          </div>
          {scanRows.length === 0 ? (
            <div className="dim3" style={{ fontSize: 12, padding: '8px 0' }}>{t('mcp.scanEmpty')}</div>
          ) : (
            <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
              {scanRows.map((r, i) => (
              <label
                key={`${r.origin}:${r.name}:${i}`}
                style={{
                  display: 'flex', alignItems: 'center', gap: 8, padding: '5px 0',
                  fontSize: 12, opacity: r.conflict ? 0.5 : 1,
                  cursor: r.conflict ? 'default' : 'pointer',
                }}
              >
                <input
                  type="checkbox"
                  disabled={r.conflict}
                  checked={picked.has(i)}
                  onChange={(e) => {
                    const n = new Set(picked)
                    if (e.target.checked) n.add(i); else n.delete(i)
                    setPicked(n)
                  }}
                />
                <span style={{ fontWeight: 510 }}>{r.name}</span>
                <span className="chip" style={{ fontSize: 9 }}>{r.origin}</span>
                {r.transport === 'remote' && (
                  <span className="chip warn" style={{ fontSize: 9 }}>{t('mcp.remote')}</span>
                )}
                {Object.keys(r.headers).length > 0 && (
                  <span className="chip" style={{ fontSize: 9 }}>
                    {t('mcp.headersN', { n: Object.keys(r.headers).length })}
                  </span>
                )}
                {r.conflict && <span className="chip warn" style={{ fontSize: 9 }}>{t('mcp.conflict')}</span>}
                <span className="dim3 mono" style={{ fontSize: 10, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {r.transport === 'remote' ? r.url : `${r.command} ${r.args.join(' ')}`}
                </span>
              </label>
              ))}
            </div>
          )}
          <div style={{ display: 'flex', gap: 8, marginTop: 8, flexShrink: 0 }}>
            <button
              className="btn"
              style={{ fontSize: 12 }}
              onClick={() => setPicked(new Set(scanRows.map((_, i) => i).filter((i) => !scanRows[i].conflict)))}
            >
              {t('mcp.pickAll')}
            </button>
            <button className="btn primary" style={{ fontSize: 12 }} disabled={busy || picked.size === 0} onClick={doImport}>
              {t('mcp.importN', { n: picked.size })}
            </button>
            <button className="btn" style={{ fontSize: 12 }} onClick={() => setScanRows(null)}>
              {t('agent.cancel')}
            </button>
          </div>
        </div>
      )}
      <div className="dim3" style={{ fontSize: 12, marginTop: 10, lineHeight: 1.7 }}>
        {t('mcp.grantsHint')}
      </div>
    </div>
  )
}

/** 全局 MCP 服务表单（票 05/票 02 三栏）：stdio(command+args+env+cwd) 与
 *  remote(url) 互斥——后端校验同款。env 用 `K=V` 行编辑（明文文件惯例同
 *  cursor/claude）。readOnly = 项目层条目只读展示（项目文件手改，全局
 *  写路径不碰它——同名覆写会错误地把项目条目抄进全局）。 */
function McpServiceForm({
  entry,
  readOnly = false,
  onDone,
  onCancel,
  onDelete,
}: {
  entry: McpEntryRow | null
  readOnly?: boolean
  onDone: () => void
  onCancel: () => void
  onDelete?: () => void
}) {
  const { t } = useTranslation()
  const { pushToast } = useUiStore()
  const [name, setName] = useState(entry?.name ?? '')
  const [remote, setRemote] = useState(entry?.transport === 'remote')
  const [command, setCommand] = useState(entry?.command ?? '')
  const [args, setArgs] = useState((entry?.args ?? []).join(' '))
  const [cwd, setCwd] = useState(entry?.cwd ?? '')
  const [url, setUrl] = useState(entry?.url ?? '')
  const [envText, setEnvText] = useState(
    Object.entries(entry?.env ?? {}).map(([k, v]) => `${k}=${v}`).join('\n'),
  )
  const [headersText, setHeadersText] = useState(
    Object.entries(entry?.headers ?? {}).map(([k, v]) => `${k}=${v}`).join('\n'),
  )
  const [disabled, setDisabled] = useState(entry?.disabled ?? false)
  const [busy, setBusy] = useState(false)

  const save = async () => {
    setBusy(true)
    try {
      const env: Record<string, string> = {}
      for (const line of envText.split('\n')) {
        const i = line.indexOf('=')
        if (i > 0) env[line.slice(0, i).trim()] = line.slice(i + 1).trim()
      }
      const headers: Record<string, string> = {}
      for (const line of headersText.split('\n')) {
        const i = line.indexOf('=')
        if (i > 0) headers[line.slice(0, i).trim()] = line.slice(i + 1).trim()
      }
      await api.saveMcpService({
        name: name.trim(),
        command: remote ? '' : command.trim(),
        args: remote ? [] : args.split(/\s+/).filter(Boolean),
        cwd: !remote && cwd.trim() ? cwd.trim() : null,
        env,
        headers,
        disabled,
        url: remote && url.trim() ? url.trim() : null,
      })
      pushToast(t('mcp.saved'), 'ok')
      onDone()
    } catch (e) {
      pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  const fld: React.CSSProperties = { marginTop: 8 }
  return (
    // ui-polish-3：右列详情统一 = .panel 圆角边线卡、满栏宽（owner 裁决，
    // 否决了上一轮的 560 限宽——四处详情宽度不一致被列为观感 bug）
    <div className="panel" style={{ marginTop: 8, padding: '12px 14px' }}>
      <fieldset disabled={readOnly} style={{ border: 'none', margin: 0, padding: 0, minWidth: 0 }}>
        <div style={fld}>
          <div className="dim3" style={{ fontSize: 10 }}>{t('mcp.fName')}</div>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} disabled={!!entry} />
        </div>
        <div style={{ ...fld, display: 'flex', gap: 12, alignItems: 'center' }}>
          <label style={{ fontSize: 12, display: 'flex', gap: 4, alignItems: 'center' }}>
            <input type="radio" checked={!remote} onChange={() => setRemote(false)} /> stdio
          </label>
          <label style={{ fontSize: 12, display: 'flex', gap: 4, alignItems: 'center' }}>
            <input type="radio" checked={remote} onChange={() => setRemote(true)} /> remote (url)
          </label>
          <label style={{ fontSize: 12, display: 'flex', gap: 4, alignItems: 'center', marginLeft: 'auto' }}>
            <input type="checkbox" checked={disabled} onChange={(e) => setDisabled(e.target.checked)} />
            {t('mcp.disabledTag')}
          </label>
        </div>
        {remote ? (
          <>
            <div style={fld}>
              <div className="dim3" style={{ fontSize: 10 }}>URL (sse/http)</div>
              <input className="input" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" />
              <div className="dim3" style={{ fontSize: 10, marginTop: 2 }}>{t('mcp.remoteHint')}</div>
            </div>
            <div style={fld}>
              <div className="dim3" style={{ fontSize: 10 }}>{t('mcp.fHeaders')}</div>
              <textarea
                className="input"
                rows={3}
                value={headersText}
                onChange={(e) => setHeadersText(e.target.value)}
                placeholder={'Authorization=Bearer …\nX-Key=…'}
                style={{ fontFamily: 'var(--mono)', fontSize: 12 }}
              />
            </div>
          </>
        ) : (
          <>
            <div style={fld}>
              <div className="dim3" style={{ fontSize: 10 }}>{t('mcp.fCommand')}</div>
              <input className="input" value={command} onChange={(e) => setCommand(e.target.value)} placeholder="npx / binary" />
            </div>
            <div style={fld}>
              <div className="dim3" style={{ fontSize: 10 }}>{t('mcp.fArgs')}</div>
              <input className="input" value={args} onChange={(e) => setArgs(e.target.value)} placeholder="-y pkg …" />
            </div>
            <div style={fld}>
              <div className="dim3" style={{ fontSize: 10 }}>{t('mcp.fCwd')}</div>
              <input className="input" value={cwd} onChange={(e) => setCwd(e.target.value)} />
            </div>
            <div style={fld}>
              <div className="dim3" style={{ fontSize: 10 }}>{t('mcp.fEnv')}</div>
              <textarea
                className="input"
                rows={3}
                value={envText}
                onChange={(e) => setEnvText(e.target.value)}
                placeholder={'KEY=value\nOTHER=…'}
                style={{ fontFamily: 'var(--mono)', fontSize: 12 }}
              />
            </div>
          </>
        )}
      </fieldset>
      {readOnly ? (
        <div className="dim3" style={{ fontSize: 10, marginTop: 8 }}>{t('mcp.projectHint')}</div>
      ) : (
        <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
          <button className="btn primary" style={{ fontSize: 12 }} disabled={busy || !name.trim()} onClick={save}>
            {t('mcp.save')}
          </button>
          {onDelete && (
            <button className="btn danger" style={{ fontSize: 12 }} disabled={busy} onClick={onDelete}>
              {t('mcp.del')}
            </button>
          )}
          <button className="btn" style={{ fontSize: 12 }} onClick={onCancel}>{t('agent.cancel')}</button>
        </div>
      )}
    </div>
  )
}

/** 自治不再分档（ADR 0069）。这里只留审查者 shadow/live。 */
function AutonomySection() {
  const { t } = useTranslation()
  const { pushToast, askConfirm } = useUiStore()
  const [reviewer, setReviewer] = useState<string | null>(null)

  useEffect(() => {
    api.reviewerMode().then(setReviewer).catch((e) => pushToast(errText(e), 'err'))
  }, [pushToast])

  const setMode = (mode: string) => {
    if (mode === reviewer) return
    const apply = async () => {
      await api.setReviewerMode(mode).then(setReviewer.bind(null, mode)).catch((e) => pushToast(errText(e), 'err'))
    }
    if (mode === 'live') {
      askConfirm({
        title: t('auto.liveTitle'),
        body: t('auto.liveBody'),
        danger: true,
        confirmLabel: t('auto.liveConfirm'),
        run: apply,
      })
    } else {
      void apply()
    }
  }

  return (
    <div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10, lineHeight: 1.7 }}>
        {t('auto.floorsHint')}
      </div>
      <Row label={t('auto.reviewer')} hint={t('auto.reviewerHint')}>
        <div style={{ display: 'flex', gap: 4 }}>
          {(['shadow', 'live'] as const).map((m) => (
            <button
              key={m}
              className={`btn ${reviewer === m ? 'primary' : ''}`}
              onClick={() => setMode(m)}
            >
              {t(`auto.mode_${m}`)}
            </button>
          ))}
        </div>
      </Row>

    </div>
  )
}

/** 用量分区（ui-audit-2 票 05 / report A5）：直接复用右栏 UsageTab——
 *  汇总/预算条/限额/分角色分解一份实现。「详情」在设置上下文里回工作台
 *  开用量明细 tab（onDetail 回调由 App 注入）。 */
function UsageSection({ onDetail }: { onDetail?: () => void }) {
  const { refreshSlow } = useUiStore()
  useEffect(() => { void refreshSlow(['usage', 'team']) }, [refreshSlow])
  return <UsageTab onDetail={onDetail} />
}

/// 诊断分类四档（词表定名，core diag.rs 同源）。显示走 i18n，
/// 发往 core 的 class 参数是字面量本身——记录里存的就是这四个词。
const DIAG_CLASSES = ['判定', '拒绝', '槽位', '宿主'] as const
const diagClassKey = (c: string): string =>
  c === '判定' ? 'judge' : c === '拒绝' ? 'reject' : c === '槽位' ? 'slot' : 'host'

/** diagnostic-records 票 01：「日志」分区——诊断开关 + 四类筛选 +
 *  结构化记录读回。记录的唯一存储是 core 的 diagnostics.jsonl，本页
 *  读回同一文件（不是第二份）。projectless：只出宿主记录；选中非宿主
 *  分类时提示先进入一个项目——这类记录按项目归，提示比空列表诚实。 */
/// 行身份：全字段合成。ts 只到秒，同秒同分支的两条会在合成里仍撞上
/// trace/activation；真正全同的两行内容可互换，共享展开态无害。
const rowKey = (r: DiagRecord): string =>
  `${r.ts}|${r.class}|${r.branch}|${r.code}|${r.project ?? ''}|${r.agent ?? ''}|${r.activation ?? ''}|${r.trace ?? ''}|${r.ms}`

function LogsSection({ projectless = false }: { projectless?: boolean }) {
  const { t } = useTranslation()
  const [logOn, setLogOn] = useState(true)
  const [cls, setCls] = useState<string | null>(null)
  const [rows, setRows] = useState<DiagRecord[]>([])
  const [open, setOpen] = useState<string | null>(null)
  // core 项目 id 恒为 PROJECT_ID('p1')；无项目上下文传 null → 只剩宿主。
  const project = projectless ? null : 'p1'

  const refresh = useCallback(() => {
    api
      .diagnosticRecords(project, cls)
      .then(setRows)
      .catch(() => setRows([]))
  }, [project, cls])

  useEffect(() => {
    api.logEnabled().then(setLogOn).catch(() => {})
  }, [])
  useEffect(() => {
    refresh()
  }, [refresh])

  const needProject = projectless && cls !== null && cls !== '宿主'

  // 布局：开关与筛选条钉在顶部不滚（外层容器对 logs 是 flex 列），
  // 滚动只发生在下方记录列表内。
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <Row label={t('settings.logging')} hint={t('settings.loggingHint')}>
        <input
          type="checkbox"
          checked={logOn}
          onChange={async (e) => {
            setLogOn(e.target.checked)
            await api.setLogEnabled(e.target.checked).catch(() => {})
            refresh()
          }}
        />
      </Row>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', padding: '12px 0 6px', flexShrink: 0 }}>
        {([null, ...DIAG_CLASSES] as (string | null)[]).map((c) => (
          <button
            key={c ?? 'all'}
            className={`btn ${cls === c ? 'primary' : ''}`}
            style={{ fontSize: 11 }}
            onClick={() => {
              setCls(c)
              setOpen(null)
            }}
          >
            {c === null ? t('settings.logs_all') : t(`settings.logs_class_${diagClassKey(c)}`)}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <button className="btn" style={{ fontSize: 11 }} onClick={refresh}>
          {t('settings.logs_refresh')}
        </button>
      </div>
      <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
        {needProject ? (
          <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center' }}>
            {t('settings.logs_needProject')}
          </div>
        ) : rows.length === 0 ? (
          <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center' }}>
            {t('settings.logs_empty')}
          </div>
        ) : (
          <div>
          {rows.map((r) => (
            // 行身份 = 全字段合成，不用序号——刷新后新记录顶到前面
            // 时，序号身份会把展开态挂到别行头上。
            <div key={rowKey(r)} style={{ borderBottom: '1px solid var(--border)' }}>
              <button
                className="btn"
                style={{
                  width: '100%', display: 'flex', gap: 8, alignItems: 'center',
                  border: 'none', padding: '7px 4px', textAlign: 'left',
                }}
                onClick={() => setOpen(open === rowKey(r) ? null : rowKey(r))}
              >
                <span className="dim3" style={{ fontSize: 11, width: 60, flexShrink: 0 }}>
                  {localLogTime(r.ts, document.documentElement.lang || navigator.language)}
                </span>
                <span className={`chip ${r.level === 'warn' ? 'err' : ''}`} style={{ fontSize: 10 }}>
                  {t(`settings.logs_class_${diagClassKey(r.class)}`)}
                </span>
                <span style={{ flex: 1, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {r.branch} · {r.code}
                </span>
                <span className="dim3" style={{ fontSize: 11, flexShrink: 0 }}>{r.ms}ms</span>
              </button>
              {open === rowKey(r) && (
                <div className="dim3" style={{ fontSize: 11, lineHeight: 1.9, padding: '2px 4px 10px 70px' }}>
                  <div>{r.ts}</div>
                  <div>{t('settings.logs_f_project')}: {r.project ?? '—'}</div>
                  <div>{t('settings.logs_f_agent')}: {r.agent ?? '—'}</div>
                  <div>{t('settings.logs_f_activation')}: {r.activation ?? '—'}</div>
                  <div>{t('settings.logs_f_trace')}: {r.trace ?? '—'}</div>
                </div>
              )}
            </div>
          ))}
          </div>
        )}
      </div>
    </div>
  )
}

export function SettingsPage({ onBack, onOpenUsageDetail, projectless = false, initialSection = 'general' }: {
  onBack: () => void
  initialSection?: Section
  onOpenUsageDetail?: () => void
  /// 无项目上下文（启动页进入）：项目作用域分区只显示提示，不触发 IPC
  projectless?: boolean
}) {
  const { t } = useTranslation()
  const { themePref, setThemePref } = useUiStore()
  const [section, setSection] = useState<Section>(initialSection)
  const [, setKeyTick] = useState(0)
  useEffect(() => { setSection(initialSection) }, [initialSection])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (matches(event, bindingFor('desktopPanel')) || matches(event, bindingFor('browserPanel'))) setSection('perms')
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])

  /// 项目作用域分区在无项目上下文下的占位（见头部注释）
  const gated = (el: React.ReactNode): React.ReactNode =>
    projectless ? (
      <div className="dim3" style={{ fontSize: 12, padding: '24px 0', textAlign: 'center' }}>
        {t('settings.needsProject')}
      </div>
    ) : el

  const bodies: Record<Section, React.ReactNode> = {
    general: (
      <>
        <Row label={t('settings.language')}>
          <select className="input" style={{ width: 'auto', minWidth: 160 }} value={i18n.language} onChange={(e) => setLang(e.target.value as Locale)}>
            {SUPPORTED.map((l) => (
              <option key={l} value={l}>{LANG_NAMES[l]}</option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.theme')}>
          <div style={{ display: 'flex', gap: 4 }}>
            {(['light', 'dark', 'night', 'system'] as ThemePref[]).map((p) => (
              <button key={p} className={`btn ${themePref === p ? 'primary' : ''}`} onClick={() => setThemePref(p)}>
                {t(`settings.theme_${p}`)}
              </button>
            ))}
          </div>
        </Row>
        <DataBoundary />
      </>
    ),
    keys: (
      <>
        <div className="dim3" style={{ fontSize: 12, marginBottom: 8 }}>{t('settings.keysHint')}</div>
        {ACTIONS.map((a) => (
          <KeyRow key={a.id} id={a.id} onChanged={() => setKeyTick((n) => n + 1)} />
        ))}
        <PracticeGround />
      </>
    ),
    team: <TeamSection projectless={projectless} />,
    models: <ProviderManager />, // 全局 providers.json，无项目也可用
    perms: <>{projectless && <DesktopPermissions />}{gated(<><DesktopControlPanel inline /><BrowserControlPanel inline /><ProjectApprovalMode placement="below" /><PermsSection /></>)}</>,
    skills: <SkillsSection />, // 全局层无项目也可用（ADR 0057）
    mcp: <McpSection projectless={projectless} />, // 全局清单无项目可配（ADR 0057）
    autonomy: gated(<AutonomySection />),

    usage: gated(<UsageSection onDetail={onOpenUsageDetail} />),
    // 日志页不被 gated 吃掉——无项目时宿主记录仍要看（diagnostic-records 票 01）。
    logs: <LogsSection projectless={projectless} />,
    // 提示词页是宿主级（翻译槽在全局 providers.json），无项目也可用（ADR 0071）。
    prompts: <PromptsSection onGoModels={() => setSection('models')} />,
    about: (
      <div style={{ fontSize: 12, lineHeight: 2 }}>
        <div><b>Hexagon-Bot</b> · v0.1.0</div>
        <div className="dim3">{t('settings.aboutLine')}</div>
      </div>
    ),
  }

  return (
    <div data-tauri-drag-region className="settings-scope" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div
        data-tauri-drag-region
        className="row-line"
        style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 14px', background: 'var(--bg-1)', paddingLeft: isMac ? '84px' : '14px' }}
      >
        <button className="btn" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }} onClick={onBack}>
          <Icon name="arrow-left" size={12} /> {t('settings.back')}
        </button>
        <strong style={{ fontWeight: 510 }}>{t('settings.title')}</strong>
        {!projectless && <DesktopPauseButton />}
      </div>
      <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
        <div style={{ width: 180, padding: '14px 10px', borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column', gap: 2 }}>
          {SECTIONS.map((s) => (
            <button
              key={s}
              className="btn"
              style={{
                textAlign: 'left', border: 'none',
                background: section === s ? 'var(--bg-2)' : 'transparent',
                color: section === s ? 'var(--text)' : 'var(--text-2)',
              }}
              onClick={() => setSection(s)}
            >
              {t(`settings.nav_${s}`)}
            </button>
          ))}
        </div>
        {/* 票 10：内容列居中；settings-density 01 owner 裁决——list|detail 分区去 cap 满宽流式（Cherry 同款），平铺表单分区保留 760 居中 */}
        {/* 日志/提示词分区例外：容器不滚——标题/工具行钉住，滚动只发生在内部列表与详情内 */}
        <div
          style={{
            flex: 1, padding: '18px 28px', maxWidth: WIDE_SECTIONS.has(section) ? 'none' : 760, margin: '0 auto', width: '100%',
            ...(section === 'logs' || section === 'prompts'
              ? { display: 'flex', flexDirection: 'column', overflowY: 'hidden' }
              : { overflowY: 'auto' }),
          }}
        >
          <div style={{ fontWeight: 560, fontSize: 15, marginBottom: 12, flexShrink: 0 }}>{t(`settings.nav_${section}`)}</div>
          {bodies[section]}
        </div>
      </div>
    </div>
  )
}
