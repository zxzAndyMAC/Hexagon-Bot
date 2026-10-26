// 设置·发布（owner-followups-1015 #14，components E14 补设计；ADR 0093：推送 ≠ 发布）。项目级：登记发布目标
// （名称、命令行、工作目录、所需凭据名）＝ E2 列表 | 详情。凭据值用密文框录入，经宿主进钥匙串；读面只回「已存入 /
// 未设置」，存后不回显、输入框随即清空。替换 = 再录一次；移除走 A17 danger 确认（首焦「取消」）。
// 只有负责人在这里登记 / 修改 / 删除；托管与 Agent 都没有这个入口（CONTEXT.md「托管」）。
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ReleaseTarget } from '../../api'
import { useUiStore } from '../../store'
import { useBusyAction } from '../../useBusyAction'
import { RELEASE_FOCUS_EVENT, takeReleaseFocus } from '../../releaseFocus'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { Field, Input, Textarea } from '@/components/ui/input'
import { Icon } from '../Icon'
import { DetailHead, Empty, ListDetail, SubHead } from './kit'

const missing = (t: ReleaseTarget) => t.credentials.filter((c) => !c.set).length

/** 登记 / 修改表单：保存后回调新的读面（新建时由它带出 id，凭据区随即可用）。 */
function TargetForm({ target, onSaved, onCancel }: {
  target: ReleaseTarget | null
  onSaved: (t: ReleaseTarget) => void
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const [name, setName] = useState(target?.name ?? '')
  const [command, setCommand] = useState(target?.command ?? '')
  const [cwd, setCwd] = useState(target?.cwd ?? '')
  const [creds, setCreds] = useState((target?.credentials ?? []).map((c) => c.name).join('\n'))
  const { busy, run } = useBusyAction()

  const save = () => run(async () => {
    const saved = await api.saveReleaseTarget({
      ...(target ? { id: target.id } : {}),
      name, command, cwd,
      credentials: creds.split('\n').map((c) => c.trim()).filter(Boolean),
    })
    pushToast(t('release.saved'), 'ok')
    onSaved(saved)
  })

  return (
    <div data-release-form className="flex flex-col gap-3">
      <Field label={t('release.fName')}>
        <Input aria-label={t('release.fName')} value={name} onChange={(e) => setName(e.target.value)} placeholder="npm" />
      </Field>
      <Field label={t('release.fCommand')} help={t('release.commandHelp')}>
        <Input aria-label={t('release.fCommand')} className="font-mono text-[12px]" value={command}
          onChange={(e) => setCommand(e.target.value)} placeholder="npm publish" />
      </Field>
      <Field label={t('release.fCwd')} help={t('release.cwdHelp')}>
        <Input aria-label={t('release.fCwd')} className="font-mono text-[12px]" value={cwd}
          onChange={(e) => setCwd(e.target.value)} placeholder={t('release.cwdRoot')} />
      </Field>
      <Field label={t('release.fCreds')} help={t('release.credsHelp')}>
        <Textarea aria-label={t('release.fCreds')} className="font-mono text-[12px]" value={creds}
          onChange={(e) => setCreds(e.target.value)} placeholder="NPM_TOKEN" />
      </Field>
      <div className="flex items-center gap-2">
        <span className="flex-1" />
        <Button size="sm" variant="quiet" onClick={onCancel}>{t('agent.cancel')}</Button>
        <Button size="sm" variant="primary" data-release-save disabled={busy || !name.trim() || !command.trim()} loading={busy} onClick={save}>
          {t('release.save')}
        </Button>
      </div>
    </div>
  )
}

/** 一条凭据：名（等宽）+ 状态胶囊 + 密文框 + 存入 / 替换 + 已存时「移除」。 */
/** 凭据的存入 / 移除：存后清空密文框；移除走 A17 danger 确认。 */
function useCredentialActions(target: ReleaseTarget, name: string, onChanged: (t: ReleaseTarget) => void) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const askConfirm = useUiStore((s) => s.askConfirm)
  const [secret, setSecret] = useState('')
  const { busy, run } = useBusyAction()

  const store = () => run(async () => {
    const next = await api.setReleaseCredential(target.id, name, secret)
    setSecret('') // 存后不回显：框清空，读面只剩「已存入」
    pushToast(t('release.credStored', { name }), 'ok')
    onChanged(next)
  })
  const remove = () => askConfirm({
    title: t('release.removeCredTitle', { name }),
    body: t('release.removeCredBody'),
    danger: true,
    confirmLabel: t('release.removeCred'),
    skipRefresh: true,
    run: async () => {
      const next = await api.removeReleaseCredential(target.id, name)
      pushToast(t('release.credRemoved', { name }), 'ok')
      onChanged(next)
    },
  })

  return { secret, setSecret, busy, store, remove }
}

function CredentialRow({ target, name, set, onChanged }: {
  target: ReleaseTarget
  name: string
  set: boolean
  onChanged: (t: ReleaseTarget) => void
}) {
  const { t } = useTranslation()
  const { secret, setSecret, busy, store, remove } = useCredentialActions(target, name, onChanged)

  return (
    <div data-release-cred={name} className="border-t border-line-soft px-3.5 py-3 first:border-t-0">
      <div className="mb-2 flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate font-mono text-[12.5px] text-ink">{name}</span>
        <Badge variant={set ? 'ok' : 'warn'} className="h-[18px] px-1.5 text-[11px]">
          {set ? t('release.credSet') : t('release.credMissing')}
        </Badge>
      </div>
      <div className="flex items-center gap-2">
        <Input
          aria-label={t('release.secretFor', { name })}
          className="min-w-0 flex-1 font-mono"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={secret}
          placeholder={set ? t('release.secretReplace') : ''}
          onChange={(e) => setSecret(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && secret.trim() && !busy) { e.preventDefault(); void store() } }}
        />
        <Button size="sm" data-release-store disabled={busy || !secret.trim()} loading={busy} onClick={() => void store()}>
          {set ? t('release.replace') : t('release.store')}
        </Button>
        {set && (
          <Button size="sm" variant="ghost" className="text-red-ink" data-release-remove-cred disabled={busy} onClick={remove}>
            {t('release.removeCred')}
          </Button>
        )}
      </div>
    </div>
  )
}

/** 选中目标的详情：表头（删除）+ 登记表单 + 凭据区。 */
function TargetDetail({ target, onChanged, onDelete, onClose }: {
  target: ReleaseTarget
  onChanged: (t: ReleaseTarget) => void
  onDelete: () => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  return (
    <div data-release-detail>
      <DetailHead
        lead={<Icon name="publish" size={18} className="flex-none text-ink-2" />}
        title={target.name}
        sub={target.cwd || t('release.cwdRoot')}
        mono={Boolean(target.cwd)}
        actions={
          <Button size="sm" variant="ghost" className="text-red-ink" data-release-delete onClick={onDelete}>
            <Icon name="trash" size={14} />{t('release.delete')}
          </Button>
        }
      />
      <div className="mt-5">
        {/* key=id：表单字段由 useState 首挂载初始化，换行要换表单 */}
        <TargetForm key={target.id} target={target} onSaved={onChanged} onCancel={onClose} />
      </div>
      <SubHead>{t('release.credsTitle')}</SubHead>
      {target.credentials.length === 0 ? (
        <div className="py-2 text-[12.5px] text-ink-3">{t('release.credsNone')}</div>
      ) : (
        <div className="rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
          {target.credentials.map((c) => (
            <CredentialRow key={`${target.id}:${c.name}`} target={target} name={c.name} set={c.set} onChanged={onChanged} />
          ))}
        </div>
      )}
    </div>
  )
}

/** 列表行：缺凭据时琥珀点 + 「缺 N 项」。 */
function TargetItem({ target }: { target: ReleaseTarget }) {
  const { t } = useTranslation()
  const n = missing(target)
  return (
    <>
      <Dot tone={n > 0 ? 'warn' : 'neutral'} />
      <div className="min-w-0 flex-1">
        <div className="truncate">{target.name}</div>
        <div className="truncate font-mono text-[12px] text-ink-3">{target.command}</div>
      </div>
      {n > 0 && <Badge variant="warn" className="h-[18px] px-1.5 text-[11px]">{t('release.missingN', { n })}</Badge>}
    </>
  )
}

/** 发布目标读面 + 选中 / 新建状态 + 删除；外部「定位到某目标」事件也落在这里。 */
function useReleaseTargets() {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const askConfirm = useUiStore((s) => s.askConfirm)
  const [targets, setTargets] = useState<ReleaseTarget[] | null>(null)
  const [error, setError] = useState('')
  // owner-followups-1015 #15：托管报告「配置发布目标」/ 发布卡「打开设置·发布」带着目标 id 进来时定位到那一行。
  const [sel, setSel] = useState<string | null>(() => takeReleaseFocus())
  const [creating, setCreating] = useState(false)
  useEffect(() => {
    const focus = (event: Event) => {
      takeReleaseFocus()
      setCreating(false)
      setSel(String((event as CustomEvent<unknown>).detail ?? '') || null)
    }
    window.addEventListener(RELEASE_FOCUS_EVENT, focus)
    return () => window.removeEventListener(RELEASE_FOCUS_EVENT, focus)
  }, [])

  const load = useCallback(() => api.releaseTargets().then(
    (rows) => { setError(''); setTargets(rows) },
    (e: unknown) => { setTargets([]); setError(errText(e)) },
  ), [])
  useEffect(() => { void load() }, [load])

  /** 用门面回的新读面替换本地一条（保存 / 录入 / 移除后不必整表重读）。 */
  const upsert = (next: ReleaseTarget) =>
    setTargets((list) => {
      const rows = list ?? []
      return rows.some((r) => r.id === next.id) ? rows.map((r) => (r.id === next.id ? next : r)) : [...rows, next]
    })

  const selected = targets?.find((r) => r.id === sel) ?? null

  const removeTarget = (target: ReleaseTarget) => askConfirm({
    title: t('release.deleteTitle', { name: target.name }),
    body: t('release.deleteBody'),
    danger: true,
    confirmLabel: t('release.delete'),
    skipRefresh: true,
    run: async () => {
      await api.removeReleaseTarget(target.id)
      setTargets((list) => (list ?? []).filter((r) => r.id !== target.id))
      setSel(null)
      pushToast(t('release.deleted', { name: target.name }), 'ok')
    },
  })

  return { targets, error, sel, setSel, creating, setCreating, load, upsert, selected, removeTarget }
}

export function ReleaseSection() {
  const { t } = useTranslation()
  const { targets, error, sel, setSel, creating, setCreating, load, upsert, selected, removeTarget } = useReleaseTargets()

  return (
    <ListDetail<ReleaseTarget>
      label={t('settings.nav_release')}
      items={targets ?? []}
      loading={targets === null}
      loadingLabel={t('release.loading')}
      error={error ? { text: error, onRetry: () => void load() } : undefined}
      itemKey={(r) => r.id}
      filterText={(r) => `${r.name} ${r.command}`}
      selected={creating ? null : sel}
      onSelect={(k) => { setSel(k); setCreating(false) }}
      searchPlaceholder={t('release.filter')}
      emptyText={t('release.empty')}
      foot={
        <Button size="xs" className="flex-1" data-release-new onClick={() => { setCreating(true); setSel(null) }}>
          <Icon name="plus" size={14} />{t('release.new')}
        </Button>
      }
      renderItem={(r) => <TargetItem target={r} />}
    >
      <div className="max-w-[600px]">
        {creating ? (
          <>
            <div className="mb-4 text-[15px] font-semibold text-ink">{t('release.new')}</div>
            <TargetForm
              target={null}
              onSaved={(saved) => { upsert(saved); setCreating(false); setSel(saved.id) }}
              onCancel={() => setCreating(false)}
            />
          </>
        ) : selected ? (
          <TargetDetail target={selected} onChanged={upsert} onDelete={() => removeTarget(selected)} onClose={() => setSel(null)} />
        ) : (
          <Empty icon="publish" title={t('settings.nav_release')} desc={t('release.intro')} />
        )}
        <p className="mt-5 text-[12px] leading-[1.6] text-ink-3">{t('release.storageNote')}</p>
        {creating && <p className="mt-1.5 text-[12px] leading-[1.6] text-ink-3">{t('release.credsAfterSave')}</p>}
      </div>
    </ListDetail>
  )
}
