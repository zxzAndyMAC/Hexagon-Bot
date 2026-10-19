// 设置「技能」分区（ui-audit-2 票 04 → global-config 票 03 双栏；ui-redesign 票 11 换皮：settings#skills，
// components E2 + E6）。左 = 已装列表（筛选 + 来源徽标 + 行内开关，>100 条虚拟列表），右 = 详情：
// 头（名 + 来源 + 文件分段 + 编辑）、SKILL.md 渲染、项目技能附经验条目。projectless 时 list_skills 回落全局层。
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, isTauri, type ExtSkillRow, type SkillRow } from '../../api'
import type { ProjectSkillDocument } from '../../gen/ProjectSkillDocument'
import { bindingFor, formatBinding, matches } from '../../keymap'
import { useUiStore } from '../../store'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, Input, Textarea } from '@/components/ui/input'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { Switch } from '@/components/ui/switch'
import { DropdownMenu, DropdownMenuContent, DropdownMenuFooter, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { ExperienceEntries } from '../ExperienceEntries'
import { Icon } from '../Icon'
import { LoadingState } from '../LoadingState'
import { CodeBlock, Md } from '../Md'
import { DetailHead, Empty, ListDetail, ScanPanel } from './kit'
import { useBusyAction } from '../../useBusyAction'

/** 来源徽标（settings#skills）：项目 info / 全局 neutral / 内置 line。 */
export function OriginBadge({ origin, label }: { origin: string; label: string }) {
  return (
    <Badge variant={origin === 'project' ? 'info' : origin === 'builtin' ? 'line' : 'neutral'} className="h-[18px] px-1.5 text-[11px]">
      {label}
    </Badge>
  )
}

/** 技能详情（global-config 票 03）：文件切换 + SKILL.md 渲染；全局技能可「编辑」成表单
 *  （名称 / 描述 / 正文 → save_global_skill 落 ~/.hexagon/skills/<name>/）；项目技能走带版本的项目命令。内置只读。 */
function SkillDetail({ skill, onSaved }: { skill: SkillRow; onSaved: () => void }) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
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
  const { busy, run } = useBusyAction()

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
    await run(async () => {
      // body 是整份 SKILL.md 原文——直接落盘（不重组 frontmatter，用户可见的就是写的）。
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
    })
  }

  // 取消编辑：字段回到最后加载的 SKILL.md 原文，放弃未保存修改。
  const cancelEdit = () => {
    setDesc(skill.description)
    setBody(origBody)
    setEditing(false)
  }

  const isMd = sel.endsWith('.md')
  const editable = skill.origin === 'global' || skill.origin === 'project'
  const editTip = skill.origin === 'project' ? `${t('skills.edit')} (${formatBinding(bindingFor('editProjectSkill'))})` : undefined
  // 负责人反馈 2026-09：详情全屏滚动、编辑区过矮——外层 ld-detail 承担滚动，编辑态正文框撑到 40vh。
  return (
    <div data-skill-detail={skill.name} onKeyDown={(e) => {
      if (skill.origin !== 'project' || e.repeat || busy) return
      if (matches(e.nativeEvent, bindingFor('editProjectSkill'))) { e.preventDefault(); e.stopPropagation(); void toggleEdit() }
      else if (editing && matches(e.nativeEvent, bindingFor('saveProjectSkill'))) { e.preventDefault(); e.stopPropagation(); void save() }
    }}>
      <DetailHead
        title={skill.name}
        sub={<OriginBadge origin={skill.origin} label={t(`skills.origin_${skill.origin}`)} />}
        actions={
          <>
            {files.length > 1 && !editing && (
              <Segmented aria-label={t('settingsUi.skillFiles')} className="max-w-[320px] overflow-x-auto" value={sel} onValueChange={setSel}>
                {files.map((f) => <SegmentedItem key={f} value={f} className="font-mono">{f}</SegmentedItem>)}
              </Segmented>
            )}
            {editable && (
              <Button size="sm" onClick={() => void toggleEdit()} title={editTip}>
                <Icon name={editing ? 'vision' : 'edit'} size={14} />{editing ? t('skills.view') : t('skills.edit')}
              </Button>
            )}
          </>
        }
      />
      {readError && <p role="alert" className="mt-3 text-[12.5px] text-red-ink">{readError}</p>}
      {reading && <div className="mt-3"><LoadingState label={t('skills.loadingDetail')} /></div>}
      {!!skill.legacy_experience_blocks && (
        <p role="status" className="mt-3 text-[12.5px] text-orange-ink">{t('skills.legacyExperience', { count: skill.legacy_experience_blocks })}</p>
      )}
      {editing ? (
        <div data-skill-editor className="mt-4 flex flex-col gap-3">
          <Field label={t('skills.descLabel')}>
            <Input aria-label={t('skills.descLabel')} value={desc} disabled={skill.origin === 'project'} onChange={(e) => setDesc(e.target.value)} />
          </Field>
          <Field label="SKILL.md">
            <Textarea aria-label="SKILL.md" value={body} onChange={(e) => setBody(e.target.value)} className="min-h-[40vh] font-mono text-[12.5px]" />
          </Field>
          <div className="flex gap-2">
            <span className="flex-1" />
            <Button size="sm" variant="quiet" disabled={busy} onClick={cancelEdit}>{t('skills.cancel')}</Button>
            <Button size="sm" variant="primary" disabled={busy} loading={busy} onClick={save}
              title={skill.origin === 'project' ? `${t('skills.save')} (${formatBinding(bindingFor('saveProjectSkill'))})` : undefined}>
              {t('skills.save')}
            </Button>
          </div>
        </div>
      ) : !reading && !readError && (
        <div data-skill-body className="mt-3 rounded-[var(--r-card)] bg-inset px-4 py-3.5 text-[13px] leading-[1.7] shadow-[inset_0_0_0_1px_var(--line-soft)]">
          {isMd ? <Md>{skillMarkdownBody(content)}</Md> : <CodeBlock code={content} />}
        </div>
      )}
      {/* 功能缺口 D3：小节头「经验条目（N）」+「历史」由 ExperienceEntries 自己出（它才知道条数）。 */}
      {skill.origin === 'project' && (
        <ExperienceEntries key={skill.name} skill={skill.name} documentRevision={readRevision}
          onRecovered={() => { setReadRevision((n) => n + 1); onSaved() }} />
      )}
    </div>
  )
}

function NewSkillForm({ onCreated }: { onCreated: (name: string) => Promise<void> }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [desc, setDesc] = useState('')
  const { busy, run } = useBusyAction()
  const create = async () => {
    if (!name.trim()) return
    await run(async () => {
      await api.saveGlobalSkill(name.trim(), desc.trim(), `# ${name.trim()}\n`)
      await onCreated(name.trim())
    })
  }
  return (
    <div data-new-skill className="flex max-w-[520px] flex-col gap-3">
      <DetailHead title={t('skills.new')} sub={t('skills.createHint')} />
      <Field label={t('skills.nameLabel')}>
        <Input aria-label={t('skills.nameLabel')} className="font-mono" placeholder="my-skill" value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label={t('skills.descLabel')}>
        <Input aria-label={t('skills.descLabel')} value={desc} onChange={(e) => setDesc(e.target.value)} />
      </Field>
      <div className="flex"><span className="flex-1" />
        <Button size="sm" variant="primary" disabled={busy || !name.trim()} loading={busy} onClick={create}>{t('skills.create')}</Button>
      </div>
    </div>
  )
}

export function SkillsSection() {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const [skills, setSkills] = useState<SkillRow[] | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const { busy, run } = useBusyAction()
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
    await run(async () => {
      const rep = await api.importSkills([...picked])
      pushToast(t('skills.importResult', { n: rep.imported, m: rep.skipped.length }), 'ok')
      setScanRows(null)
      await load()
    })
  }

  // 功能缺口 D4（原型 ld-foot「安装…」）：一个按钮开 A16 菜单（文件夹 / ZIP），替代原来并排两个且只在桌面壳
  // 出现的按钮。安装 ≠ 授权（CONTEXT.md「能力授权」）：装进来只进清单，角色要在团队里勾选才用得上——菜单脚注写明。
  // 浏览器预览没有本机文件对话框：菜单项禁用并写原因（A16 禁用行描述写原因），不静默无反应。
  const installFrom = async (directory: boolean) => {
    if (!isTauri) return
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const pickedPath = await open(directory ? { directory: true } : { filters: [{ name: 'ZIP', extensions: ['zip'] }] })
      if (typeof pickedPath !== 'string') return
      const name = await api.installSkillPath(pickedPath)
      pushToast(t('skills.installedOk', { name }), 'ok')
      await load()
      setSelected(name)
    } catch (e) { pushToast(errText(e), 'err') }
  }

  const selSkill = skills?.find((s) => s.name === selected) ?? null

  return (
    <ListDetail<SkillRow>
      label={t('settings.nav_skills')}
      virtualized
      items={skills ?? []}
      loading={loading}
      loadingLabel={t('skills.loadingList')}
      error={loadError ? { text: loadError, onRetry: () => void load(), retryLabel: t('skills.retryLoad'), retryAction: 'retrySkillsLoad' } : undefined}
      itemKey={(s) => s.name}
      filterText={(s) => `${s.name} ${s.description}`}
      selected={creating ? null : selected}
      onSelect={(k) => { setSelected(k); setCreating(false) }}
      // 功能缺口 D4：占位带总数「筛选 38 个技能」（原型）；加载中 / 失败时总数未知，回落不带数的占位。
      searchPlaceholder={skills ? t('skills.filterN', { count: skills.length }) : t('skills.filter')}
      emptyText={t('skills.empty')}
      foot={
        <>
          <Button size="xs" className="flex-1" onClick={() => { setCreating(true); setSelected(null) }}><Icon name="plus" size={14} />{t('skills.new')}</Button>
          <Button size="xs" className="flex-1" loading={scanning} onClick={scan}>{scanning ? t('skills.scanning') : t('skills.scan')}</Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="xs" className="flex-1" data-skill-install>{t('skills.install')}</Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" side="top">
              <DropdownMenuItem tall icon={<Icon name="folder" size={16} />} disabled={!isTauri}
                description={isTauri ? t('skills.installDirHint') : t('skills.installDesktopOnly')} onSelect={() => void installFrom(true)}>
                {t('skills.installDir')}
              </DropdownMenuItem>
              <DropdownMenuItem tall icon={<Icon name="install" size={16} />} disabled={!isTauri}
                description={isTauri ? t('skills.installZipHint') : t('skills.installDesktopOnly')} onSelect={() => void installFrom(false)}>
                {t('skills.installZip')}
              </DropdownMenuItem>
              <DropdownMenuFooter>{t('skills.installNote')}</DropdownMenuFooter>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
      renderItem={(s) => (
        <>
          <div className="min-w-0 flex-1">
            <div className="truncate">{s.name}</div>
            <div className="truncate text-[12px] text-ink-3">{s.description}</div>
          </div>
          <OriginBadge origin={s.origin} label={t(`skills.origin_${s.origin}`)} />
          <Switch
            aria-label={`${t('skills.enabled')} ${s.name}`}
            checked={s.enabled}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            onCheckedChange={() => toggle(s)}
          />
        </>
      )}
      below={scanRows !== null && (
        // 负责人反馈 2026-09：扫描结果平铺把整页顶出去——面板限高，标题与操作钉住，只有行列表滚。
        <ScanPanel
          title={t('skills.scanTitle')}
          hint={t('skills.scanHint')}
          rows={scanRows.length === 0 ? (
            <div className="py-2 text-[12.5px] text-ink-3">{t('skills.scanEmpty')}</div>
          ) : scanRows.map((r) => (
            <label key={r.path} className={`flex items-center gap-2 py-1.5 text-[12.5px] ${r.conflict ? 'cursor-default opacity-50' : 'cursor-pointer'}`}>
              <input type="checkbox" className="accent-[var(--accent)]" disabled={r.conflict} checked={picked.has(r.path)}
                onChange={(e) => {
                  const n = new Set(picked)
                  if (e.target.checked) n.add(r.path); else n.delete(r.path)
                  setPicked(n)
                }} />
              <span className="font-medium text-ink">{r.name}</span>
              <Badge className="h-[18px] px-1.5 text-[11px]">{r.origin}</Badge>
              {r.conflict && <Badge variant="warn" className="h-[18px] px-1.5 text-[11px]">{t('skills.conflict')}</Badge>}
              <span className="min-w-0 flex-1 truncate text-ink-3">{r.description}</span>
            </label>
          ))}
          actions={
            <>
              <Button size="xs" onClick={() => setPicked(new Set(scanRows.filter((r) => !r.conflict).map((r) => r.path)))}>{t('skills.pickAll')}</Button>
              <span className="flex-1" />
              <Button size="xs" variant="quiet" onClick={() => setScanRows(null)}>{t('agent.cancel')}</Button>
              <Button size="xs" variant="primary" disabled={busy || picked.size === 0} onClick={doImport}>{t('skills.importN', { n: picked.size })}</Button>
            </>
          }
        />
      )}
    >
      {creating ? (
        <NewSkillForm onCreated={async (name) => { setCreating(false); await load(); setSelected(name) }} />
      ) : selSkill ? (
        <SkillDetail key={selSkill.name} skill={selSkill} onSaved={load} />
      ) : (
        <Empty icon="book" title={t('settings.nav_skills')} desc={t('skills.intro')} />
      )}
    </ListDetail>
  )
}

/** 票 16B（settings#skills 详情渲染 SKILL.md 正文）：阅读态去掉开头的 YAML frontmatter——名称与描述已在
 *  详情头和列表里；不去掉时 `---` 被渲染成分隔线、`name: … description: …` 挤成一段正文。编辑态仍是整份原文。 */
export function skillMarkdownBody(content: string): string {
  return content.replace(/^\uFEFF?---\r?\n[\s\S]*?\r?\n---[ \t]*(?:\r?\n|$)/, '')
}
