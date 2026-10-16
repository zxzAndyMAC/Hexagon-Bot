// 设置「MCP 服务」分区（global-config 票 05；ui-redesign 票 11 换皮：settings#mcp，components E2 + E6）。
// 全局清单 ~/.hexagon/mcp.json ∪ 项目 .hexagon/mcp.json（同名项目覆盖），无项目可增删改全局层——配置清单 ≠ 授权
// （ADR 0010，授权仍按 Agent 走团队区 grants）。远程（url）条目可见但一期不 spawn。实况块只在有项目时渲染
// （宿主生命周期是项目级的）。删除暂用现有直删（票 13 统一换 AlertDialog）。
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ExtMcpRow, type McpEntryRow, type McpServiceRow } from '../../api'
import { useUiStore } from '../../store'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { CodeChip } from '@/components/ui/code-chip'
import { Dot } from '@/components/ui/dot'
import { Field, Input, Textarea } from '@/components/ui/input'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { Switch } from '@/components/ui/switch'
import { Icon } from '../Icon'
import { Empty, ListDetail, ScanPanel } from './kit'

const kv = (o: Record<string, string | undefined>) => Object.entries(o).map(([k, v]) => `${k}=${v ?? ''}`).join('\n')
const parseKv = (text: string) => {
  const out: Record<string, string> = {}
  for (const line of text.split('\n')) {
    const i = line.indexOf('=')
    if (i > 0) out[line.slice(0, i).trim()] = line.slice(i + 1).trim()
  }
  return out
}

/** 全局 MCP 服务表单（票 05 / 票 02 三栏）：stdio（command + args + env + cwd）与 remote（url + headers）互斥——
 *  后端校验同款。env / headers 用 `K=V` 行编辑（明文文件惯例）。readOnly = 项目层条目只读展示
 *  （项目文件手改，全局写路径不碰它——同名覆写会错误地把项目条目抄进全局）。 */
function McpServiceForm({ entry, readOnly = false, onDone, onCancel, onDelete }: {
  entry: McpEntryRow | null
  readOnly?: boolean
  onDone: () => void
  onCancel: () => void
  onDelete?: () => void
}) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const [name, setName] = useState(entry?.name ?? '')
  const [remote, setRemote] = useState(entry?.transport === 'remote')
  const [command, setCommand] = useState(entry?.command ?? '')
  const [args, setArgs] = useState((entry?.args ?? []).join(' '))
  const [cwd, setCwd] = useState(entry?.cwd ?? '')
  const [url, setUrl] = useState(entry?.url ?? '')
  const [envText, setEnvText] = useState(kv(entry?.env ?? {}))
  const [headersText, setHeadersText] = useState(kv(entry?.headers ?? {}))
  const [disabled, setDisabled] = useState(entry?.disabled ?? false)
  const [busy, setBusy] = useState(false)

  const save = async () => {
    setBusy(true)
    try {
      await api.saveMcpService({
        name: name.trim(),
        command: remote ? '' : command.trim(),
        args: remote ? [] : args.split(/\s+/).filter(Boolean),
        cwd: !remote && cwd.trim() ? cwd.trim() : null,
        env: parseKv(envText),
        headers: parseKv(headersText),
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

  return (
    <div data-mcp-form className="flex flex-col gap-3">
      <fieldset disabled={readOnly} className="m-0 flex min-w-0 flex-col gap-3 border-0 p-0">
        <Field label={t('mcp.fName')}>
          <Input aria-label={t('mcp.fName')} value={name} onChange={(e) => setName(e.target.value)} disabled={!!entry || readOnly} />
        </Field>
        <div className="flex items-center gap-3">
          <Segmented aria-label={t('settingsUi.transport')} value={remote ? 'remote' : 'stdio'} onValueChange={(v) => setRemote(v === 'remote')}>
            <SegmentedItem value="stdio" disabled={readOnly}>stdio</SegmentedItem>
            <SegmentedItem value="remote" disabled={readOnly}>{t('settingsUi.transportRemote')}</SegmentedItem>
          </Segmented>
          <span className="flex-1" />
          <label className="flex items-center gap-2 text-[12.5px] text-ink-2">
            {t('mcp.enabled')}
            <Switch aria-label={t('mcp.enabled')} checked={!disabled} disabled={readOnly} onCheckedChange={(on) => setDisabled(!on)} />
          </label>
        </div>
        {remote ? (
          <>
            <Field label="URL" help={t('mcp.remoteHint')}>
              <Input aria-label="URL" className="font-mono text-[12px]" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" />
            </Field>
            <Field label={t('mcp.fHeaders')} help={t('settingsUi.plaintextHelp')}>
              <Textarea aria-label={t('mcp.fHeaders')} className="font-mono text-[12px]" value={headersText}
                onChange={(e) => setHeadersText(e.target.value)} placeholder={'Authorization=Bearer …\nX-Key=…'} />
            </Field>
          </>
        ) : (
          <>
            <Field label={t('mcp.fCommand')}>
              <Input aria-label={t('mcp.fCommand')} className="font-mono text-[12px]" value={command} onChange={(e) => setCommand(e.target.value)} placeholder="npx / binary" />
            </Field>
            <Field label={t('mcp.fArgs')}>
              <Input aria-label={t('mcp.fArgs')} className="font-mono text-[12px]" value={args} onChange={(e) => setArgs(e.target.value)} placeholder="-y pkg …" />
            </Field>
            <Field label={t('mcp.fCwd')}>
              <Input aria-label={t('mcp.fCwd')} className="font-mono text-[12px]" value={cwd} onChange={(e) => setCwd(e.target.value)} />
            </Field>
            <Field label={t('mcp.fEnv')} help={t('settingsUi.plaintextHelp')}>
              <Textarea aria-label={t('mcp.fEnv')} className="font-mono text-[12px]" value={envText}
                onChange={(e) => setEnvText(e.target.value)} placeholder={'KEY=value\nOTHER=…'} />
            </Field>
          </>
        )}
      </fieldset>
      {readOnly ? (
        <div className="text-[12px] text-ink-3">{t('mcp.projectHint')}</div>
      ) : (
        <div className="flex items-center gap-2">
          {onDelete && (
            <Button size="sm" variant="ghost" className="text-red" disabled={busy} onClick={onDelete}>
              <Icon name="trash" size={14} />{t('mcp.del')}
            </Button>
          )}
          <span className="flex-1" />
          <Button size="sm" variant="quiet" onClick={onCancel}>{t('agent.cancel')}</Button>
          <Button size="sm" variant="primary" disabled={busy || !name.trim()} loading={busy} onClick={save}>{t('mcp.save')}</Button>
        </div>
      )}
    </div>
  )
}

/** 实况块（settings#mcp）：inset 内状态点 + 「运行中 · N 个工具」+ 工具 code chip + 说明。 */
function LiveBlock({ live }: { live: McpServiceRow }) {
  const { t } = useTranslation()
  const tone = live.status === 'up' ? 'on' : live.status === 'starting' ? 'warn' : 'err'
  return (
    <div data-mcp-live className="mt-4 rounded-[var(--r-card)] bg-inset p-3 shadow-[inset_0_0_0_1px_var(--line-soft)]">
      <div className="flex items-center gap-2 text-[12px]">
        <Dot tone={tone} pulse={live.status === 'starting'} />
        <b className="min-w-0 flex-1 font-medium text-ink">
          {live.status === 'up' ? t('mcp.up', { count: live.tools.length }) : live.status === 'starting' ? t('mcp.starting') : t('mcp.down')}
        </b>
        <span className="text-ink-3">{t('mcp.live')}</span>
      </div>
      {live.status === 'up' && live.tools.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {live.tools.map((n) => <CodeChip key={n}>{n}</CodeChip>)}
        </div>
      )}
      {live.error && <div role="alert" className="mt-2 text-[12px] text-red">{live.error}</div>}
      <div className="mt-2 text-[12px] text-ink-3">{t('settingsUi.mcpToolsNote', { name: live.name })}</div>
    </div>
  )
}

export function McpSection({ projectless }: { projectless: boolean }) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const [rows, setRows] = useState<McpServiceRow[] | null>(null)
  const [entries, setEntries] = useState<McpEntryRow[] | null>(null)
  const [error, setError] = useState('')
  const [sel, setSel] = useState<string | null>(null) // `${origin}:${name}`
  const [creating, setCreating] = useState(false)
  // 票 06：本机 MCP 扫描导入
  const [scanRows, setScanRows] = useState<ExtMcpRow[] | null>(null)
  const [picked, setPicked] = useState<Set<number>>(new Set())
  const [scanning, setScanning] = useState(false)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    setError('')
    try { setEntries(await api.mcpEntries()) } catch (e) { setEntries([]); setError(errText(e)) }
  }, [])

  useEffect(() => {
    void load()
    if (!projectless) api.mcpServices().then(setRows).catch((e) => pushToast(errText(e), 'err'))
  }, [projectless, load, pushToast])

  const del = async (name: string) => {
    try {
      await api.deleteMcpService(name)
      await load()
    } catch (e) { pushToast(errText(e), 'err') }
  }

  const scan = async () => {
    setScanning(true)
    try {
      const found = await api.scanExternalMcp()
      setScanRows(found)
      setPicked(new Set(found.map((_, i) => i).filter((i) => !found[i].conflict)))
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
    try { await api.openMcpMarket() } catch (e) { pushToast(errText(e), 'err') }
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

  const liveOf = (e: McpEntryRow) => rows?.find((r) => r.name === e.name)
  const selEntry = entries?.find((e) => `${e.origin}:${e.name}` === sel) ?? null
  const selLive = selEntry ? liveOf(selEntry) : undefined
  const sub = (e: McpEntryRow) => {
    const n = Object.keys(e.headers ?? {}).length
    const transport = e.transport === 'remote' ? t('settingsUi.transportRemote') : 'stdio'
    return [transport, n > 0 ? t('mcp.headersN', { n }) : '', e.disabled ? t('mcp.disabledTag') : ''].filter(Boolean).join(' · ')
  }

  return (
    <ListDetail<McpEntryRow>
      label={t('settings.nav_mcp')}
      items={entries ?? []}
      loading={entries === null}
      error={error ? { text: error, onRetry: () => void load() } : undefined}
      itemKey={(e) => `${e.origin}:${e.name}`}
      filterText={(e) => `${e.name} ${e.command} ${e.url ?? ''}`}
      selected={creating ? null : sel}
      onSelect={(k) => { setSel(k); setCreating(false) }}
      searchPlaceholder={t('mcp.filter')}
      emptyText={t('mcp.empty')}
      foot={
        <>
          <Button size="xs" className="flex-1" onClick={() => { setCreating(true); setSel(null) }}><Icon name="plus" size={14} />{t('mcp.new')}</Button>
          <Button size="xs" className="flex-1" loading={scanning} onClick={scan}>{scanning ? t('mcp.scanning') : t('mcp.scan')}</Button>
          {/* 功能缺口 D5（原型 ld-foot「市场」）：不再只在桌面壳出现——open_mcp_market 宿主命令早已注册（外部浏览器打开
              市场页），浏览器预览走 mock。title 写明去向：会离开应用打开外部网页。 */}
          <Button size="xs" className="flex-1" data-mcp-market title={t('mcp.marketTip')} onClick={openMarket}><Icon name="external" size={14} />{t('mcp.market')}</Button>
        </>
      }
      renderItem={(s) => {
        const live = liveOf(s)
        return (
          <>
            <Dot tone={s.disabled ? 'off' : live?.status === 'up' ? 'on' : live?.status === 'down' ? 'err' : 'neutral'} />
            <div className="min-w-0 flex-1">
              <div className="truncate">{s.name}</div>
              <div className="truncate text-[12px] text-ink-3">{sub(s)}</div>
            </div>
            <Badge variant={s.origin === 'project' ? 'info' : 'neutral'} className="h-[18px] px-1.5 text-[11px]">
              {s.origin === 'global' ? t('mcp.origin_global') : t('mcp.origin_project')}
            </Badge>
            {s.origin === 'global' && s.transport === 'stdio' && (
              <Switch aria-label={`${t('mcp.enabled')} ${s.name}`} checked={!s.disabled}
                onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}
                onCheckedChange={() => void toggle(s)} />
            )}
          </>
        )
      }}
      below={scanRows !== null && (
        // 同技能扫描面板：限高，标题 / 操作钉住，只有行列表滚（负责人反馈 2026-09）。
        <ScanPanel
          title={t('mcp.scanTitle')}
          hint={t('mcp.scanHint')}
          rows={scanRows.length === 0 ? (
            <div className="py-2 text-[12.5px] text-ink-3">{t('mcp.scanEmpty')}</div>
          ) : scanRows.map((r, i) => (
            <label key={`${r.origin}:${r.name}:${i}`} className={`flex items-center gap-2 py-1.5 text-[12.5px] ${r.conflict ? 'cursor-default opacity-50' : 'cursor-pointer'}`}>
              <input type="checkbox" className="accent-[var(--accent)]" disabled={r.conflict} checked={picked.has(i)}
                onChange={(e) => {
                  const n = new Set(picked)
                  if (e.target.checked) n.add(i); else n.delete(i)
                  setPicked(n)
                }} />
              <span className="font-medium text-ink">{r.name}</span>
              <Badge className="h-[18px] px-1.5 text-[11px]">{r.origin}</Badge>
              {r.transport === 'remote' && <Badge variant="warn" className="h-[18px] px-1.5 text-[11px]">{t('mcp.remote')}</Badge>}
              {Object.keys(r.headers ?? {}).length > 0 && (
                <Badge className="h-[18px] px-1.5 text-[11px]">{t('mcp.headersN', { n: Object.keys(r.headers ?? {}).length })}</Badge>
              )}
              {r.conflict && <Badge variant="warn" className="h-[18px] px-1.5 text-[11px]">{t('mcp.conflict')}</Badge>}
              <span className="min-w-0 flex-1 truncate font-mono text-ink-3">{r.transport === 'remote' ? r.url : `${r.command} ${r.args.join(' ')}`}</span>
            </label>
          ))}
          actions={
            <>
              <Button size="xs" onClick={() => setPicked(new Set(scanRows.map((_, i) => i).filter((i) => !scanRows[i].conflict)))}>{t('mcp.pickAll')}</Button>
              <span className="flex-1" />
              <Button size="xs" variant="quiet" onClick={() => setScanRows(null)}>{t('agent.cancel')}</Button>
              <Button size="xs" variant="primary" disabled={busy || picked.size === 0} onClick={doImport}>{t('mcp.importN', { n: picked.size })}</Button>
            </>
          }
        />
      )}
    >
      <div className="max-w-[600px]">
        {creating ? (
          <>
            <div className="mb-4 text-[15px] font-semibold text-ink">{t('mcp.new')}</div>
            <McpServiceForm entry={null} onDone={async () => { setCreating(false); await load() }} onCancel={() => setCreating(false)} />
          </>
        ) : selEntry ? (
          <>
            <div className="mb-4 flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-[15px] font-semibold text-ink">{selEntry.name}</span>
              <Badge variant={selEntry.origin === 'project' ? 'info' : 'neutral'}>
                {selEntry.origin === 'global' ? t('mcp.origin_global') : t('mcp.origin_project')}
              </Badge>
            </div>
            {/* key=sel：表单字段是 useState(entry…) 首挂载初始化——不挂 key 换行不换内容（ui-polish-2 ②） */}
            <McpServiceForm
              key={sel}
              entry={selEntry}
              readOnly={selEntry.origin !== 'global'}
              onDone={async () => { setSel(null); await load() }}
              onCancel={() => setSel(null)}
              onDelete={selEntry.origin === 'global' ? async () => { await del(selEntry.name); setSel(null) } : undefined}
            />
            {!projectless && selLive && <LiveBlock live={selLive} />}
          </>
        ) : (
          <Empty icon="plug" title={t('settings.nav_mcp')} desc={t('mcp.intro')} />
        )}
        <div className="mt-5 flex flex-col gap-1.5 text-[12px] leading-[1.6] text-ink-3">
          <p>{t('mcp.grantsHint')}</p>
          <p>{t('dataBoundary.edit')}</p>
          <p>{t('dataBoundary.mcpStorage')}</p>
        </div>
      </div>
    </ListDetail>
  )
}
