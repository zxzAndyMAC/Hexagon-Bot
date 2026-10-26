// 头像换图 / 恢复默认的共用件（owner-followups-1015 票 17，规范 components E4「头像」）。
// 三处挂载（另有成员行右键菜单走 pickAvatarFile，票 18）：画布 Agent 详情（实例层）、设置·团队 Agent 详情与向导角色详情（角色层）。
// 只管「选文件 → data URL」与「恢复默认」按钮外形；写到哪一层由调用方的 onPicked 决定。
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import i18n from '../i18n'
import { errText } from '../api'
import { useUiStore } from '../store'
import { Button } from '@/components/ui/button'
import { Icon } from './Icon'

/** 界面预检上限：与核 `avatars::MAX_AVATAR_BYTES` 同值。核仍会再判一次（中心裁方也在核里）。 */
const AVATAR_MAX_BYTES = 2 * 1024 * 1024
const ACCEPT = 'image/png,image/jpeg,image/webp,image/gif'

/**
 * 读一张图 → data URL → onPicked。>2MB 本地拒绝不发请求（data URL 会膨胀 ~4/3）；onPicked 抛错走 toast。
 * 组件内（useAvatarFile）与组件外（成员行右键 / 命令面板 / 键位，票 18 pickAvatarFile）共用。
 */
export function readAvatarFile(f: File, onPicked: (dataUrl: string) => Promise<void> | void) {
  const pushToast = useUiStore.getState().pushToast
  if (f.size > AVATAR_MAX_BYTES) { pushToast(i18n.t('agent.avatarTooBig'), 'err'); return }
  // Benchmark I1 / 2026-10-06 Spec: a delayed A FileReader previously
  // fetched B's IPC identity and wrote B's same-named agent avatar.
  const epoch = useUiStore.getState().projectEpoch
  const r = new FileReader()
  r.onload = async () => {
    if (useUiStore.getState().projectEpoch !== epoch) return
    try { await onPicked(String(r.result)) }
    catch (err) { if (useUiStore.getState().projectEpoch === epoch) pushToast(errText(err), 'err') }
  }
  r.readAsDataURL(f)
}

/** 无挂载点的换图入口（票 18）：临时文件框，选完即弃。菜单 / 命令面板关了也能用，不依赖某个组件活着。 */
export function pickAvatarFile(onPicked: (dataUrl: string) => Promise<void> | void) {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = ACCEPT
  input.dataset.avatarFile = ''
  input.addEventListener('change', () => { const f = input.files?.[0]; if (f) readAvatarFile(f, onPicked) }, { once: true })
  input.click()
}

/** 隐藏文件框 + open()。 */
export function useAvatarFile(onPicked: (dataUrl: string) => Promise<void> | void) {
  const ref = useRef<HTMLInputElement>(null)
  const input = (
    <input ref={ref} type="file" accept={ACCEPT} className="hidden" data-avatar-file
      onChange={(e) => { const f = e.target.files?.[0]; if (f) readAvatarFile(f, onPicked); e.target.value = '' }} />
  )
  return { open: () => ref.current?.click(), input }
}

/** 「恢复默认」：该层没有自定义图时不渲染（规范 E4：不是禁用）。 */
export function RestoreAvatarButton({ shown, size = 'sm', onRestore }: {
  shown: boolean
  size?: 'sm' | 'xs'
  onRestore: () => Promise<void> | void
}) {
  const { t } = useTranslation()
  if (!shown) return null
  const run = async () => {
    try { await onRestore() } catch (err) { useUiStore.getState().pushToast(errText(err), 'err') }
  }
  return (
    <Button size={size} variant="ghost" data-avatar-restore onClick={() => void run()}>
      <Icon name="close" size={size === 'xs' ? 12 : 14} />{t('avatar.restoreDefault')}
    </Button>
  )
}
