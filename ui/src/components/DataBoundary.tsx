import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import type { DataBoundary as Boundary } from '../gen/DataBoundary'
import { Badge } from '@/components/ui/badge'
import { Group, GroupTitle, SRow } from './settings/kit'

type Recipient = Boundary['recipients'][number]

/** Reliability 22: a read-only explanation of actual storage and configured exits.
 * ui-redesign 票 11（settings#general「数据去向」）：分组卡四行——模型凭据 / 模型请求 / MCP 服务 / 工作台运营方，
 * 右侧状态 chip；原有的逐条说明（本机、终端网络、保留期）原文保留在分组下方的脚注里，不删减。 */
export function DataBoundary() {
  const { t } = useTranslation()
  const [value, setValue] = useState<Boundary | null>(null)
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    let live = true
    api.dataBoundary().then((v) => { if (live) setValue(v) }).catch(() => { if (live) setFailed(true) })
    return () => { live = false }
  }, [])
  const where = (r: Recipient) =>
    r.endpoint ?? t(r.transport === 'stdio' ? 'dataBoundary.localProcess' : 'dataBoundary.unknownEndpoint')
  const line = (rs: Recipient[]) => rs.length === 0
    ? t('dataBoundary.empty')
    : rs.map((r) => `${where(r)}${r.enabled ? '' : ` (${t('dataBoundary.disabled')})`}`).join(' · ')
  const of = (kind: Recipient['kind']) => value?.recipients.filter((r) => r.kind === kind) ?? []
  const models = of('model')
  const mcp = of('mcp')
  const search = of('search')
  return (
    <section aria-label={t('dataBoundary.title')} data-data-boundary>
      <GroupTitle>{t('dataBoundary.title')}</GroupTitle>
      <Group>
        {failed ? (
          <SRow icon="warn" label={t('dataBoundary.configured')} hint={<span role="alert">{t('dataBoundary.unavailable')}</span>} />
        ) : (
          <>
            <SRow icon="key" label={t('dataBoundary.credentials')}
              hint={value ? t(`dataBoundary.backend_${value.credential_backend}`, { defaultValue: t('dataBoundary.backend_custom') }) : '…'}>
              {value && (
                <Badge variant={value.credential_backend === 'keychain' ? 'ok' : 'warn'}>
                  {t(value.credential_backend === 'keychain' ? 'settingsUi.credSecure' : 'settingsUi.credPlain')}
                </Badge>
              )}
            </SRow>
            <SRow icon="cpu" label={t('settingsUi.modelRequests')} hint={value && <span className="font-mono">{line(models)}</span>}>
              {value && <Badge>{t('settingsUi.recipients', { count: models.length })}</Badge>}
            </SRow>
            <SRow icon="plug" label={t('settings.nav_mcp')} hint={value && <span className="font-mono">{line(mcp)}</span>}>
              {value && mcp.length > 0 && <Badge variant="warn">{t('settingsUi.plaintext')}</Badge>}
            </SRow>
            {search.length > 0 && (
              <SRow icon="search" label={t('dataBoundary.kind_search')} hint={<span className="font-mono">{line(search)}</span>}>
                <Badge>{t('settingsUi.recipients', { count: search.length })}</Badge>
              </SRow>
            )}
            <SRow icon="hex" label={t('settingsUi.operator')} hint={t('settingsUi.operatorHint')}>
              <Badge variant="ok">{t('settingsUi.none')}</Badge>
            </SRow>
          </>
        )}
      </Group>
      <div data-boundary-notes className="-mt-2 mb-5 flex flex-col gap-1.5 px-1 text-[12px] leading-[1.6] text-ink-3">
        <p>{t('dataBoundary.local')}</p>
        <p>{t('dataBoundary.model')}</p>
        <p>{t('dataBoundary.mcp')}</p>
        <p>{t('dataBoundary.mcpStorage')}</p>
        <p>{t('dataBoundary.terminal')}</p>
        <p>{t('dataBoundary.retention')}</p>
        {!failed && <p>{t('dataBoundary.configured')}</p>}
      </div>
    </section>
  )
}
