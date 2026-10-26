// owner-followups-1015 #14（components E14）：设置·发布页——列表 | 详情、登记、凭据录入 / 替换 / 移除（确认框首焦取消），
// 凭据值存后不回显。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../../i18n'
import { api, type ReleaseTarget } from '../../api'
import { useUiStore } from '../../store'
import { ConfirmHost } from '../ConfirmDialog'
import { SettingsPage } from '../SettingsPage'
import { ReleaseSection } from './ReleaseSection'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const t = (k: string, o?: Record<string, unknown>) => i18n.t(k, o) as string
const npm = (set: boolean): ReleaseTarget => ({
  id: 'r1', name: 'npm', command: 'npm publish', cwd: 'packages/web',
  credentials: [{ name: 'NPM_TOKEN', set }, { name: 'NPM_OTP', set: false }],
})
const pages: ReleaseTarget = { id: 'r2', name: 'Pages', command: 'npm run deploy', cwd: '', credentials: [] }

let el: HTMLDivElement
let root: Root
beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.clear()
  vi.spyOn(api, 'releaseTargets').mockResolvedValue([npm(true), pages])
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => {
  await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks()
  useUiStore.setState({ confirmReq: null, toasts: [] })
})

const render = async () => {
  await act(async () => root.render(<><ReleaseSection /><ConfirmHost /></>))
  await act(async () => {})
}
const option = (name: string) =>
  [...el.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => o.textContent?.includes(name))!
const open = async (name: string) => {
  await act(async () => option(name).click())
}
const type = async (input: HTMLInputElement | HTMLTextAreaElement, value: string) => {
  const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
const byLabel = <T extends HTMLElement>(label: string) => el.querySelector<T>(`[aria-label="${label}"]`)!
const cred = (name: string) => el.querySelector<HTMLElement>(`[data-release-cred="${name}"]`)!

describe('设置·发布 列表 | 详情', () => {
  it('列表行只表状态（无可交互后代），缺凭据出胶囊；详情回填表单与凭据状态', async () => {
    await render()
    const options = el.querySelectorAll('[role="option"]')
    expect(options).toHaveLength(2)
    for (const o of options) expect(o.querySelector('button, input, [role="switch"]')).toBeNull()
    expect(option('npm').textContent).toContain(t('release.missingN', { n: 1 }))
    expect(option('Pages').textContent).not.toContain(t('release.missingN', { n: 1 }))

    await open('npm')
    expect(byLabel<HTMLInputElement>(t('release.fName')).value).toBe('npm')
    expect(byLabel<HTMLInputElement>(t('release.fCommand')).value).toBe('npm publish')
    expect(byLabel<HTMLInputElement>(t('release.fCwd')).value).toBe('packages/web')
    expect(byLabel<HTMLTextAreaElement>(t('release.fCreds')).value).toBe('NPM_TOKEN\nNPM_OTP')
    expect(cred('NPM_TOKEN').textContent).toContain(t('release.credSet'))
    expect(cred('NPM_OTP').textContent).toContain(t('release.credMissing'))
    // 已存入的行是「替换」+「移除」，未设置的行是「存入」、没有移除
    expect(cred('NPM_TOKEN').querySelector('[data-release-store]')!.textContent).toBe(t('release.replace'))
    expect(cred('NPM_TOKEN').querySelector('[data-release-remove-cred]')).not.toBeNull()
    expect(cred('NPM_OTP').querySelector('[data-release-store]')!.textContent).toBe(t('release.store'))
    expect(cred('NPM_OTP').querySelector('[data-release-remove-cred]')).toBeNull()
  })

  it('新建：表单按行拆凭据名，保存后选中新目标并出凭据区', async () => {
    const created: ReleaseTarget = { id: 'r3', name: 'crates', command: 'cargo publish', cwd: '', credentials: [{ name: 'CARGO_TOKEN', set: false }] }
    const save = vi.spyOn(api, 'saveReleaseTarget').mockResolvedValue(created)
    await render()
    await act(async () => el.querySelector<HTMLButtonElement>('[data-release-new]')!.click())
    await type(byLabel(t('release.fName')), 'crates')
    await type(byLabel(t('release.fCommand')), 'cargo publish')
    await type(byLabel(t('release.fCreds')), 'CARGO_TOKEN\n\n')
    await act(async () => el.querySelector<HTMLButtonElement>('[data-release-save]')!.click())
    expect(save).toHaveBeenCalledWith({ name: 'crates', command: 'cargo publish', cwd: '', credentials: ['CARGO_TOKEN'] })
    expect(el.querySelector('[data-release-detail]')!.textContent).toContain('crates')
    expect(cred('CARGO_TOKEN')).not.toBeNull()
    expect(option('crates').getAttribute('aria-selected')).toBe('true')
  })

  it('core 拒绝（release_cwd_outside）→ 原因码文案 toast', async () => {
    vi.spyOn(api, 'saveReleaseTarget').mockRejectedValue({ code: 'release_cwd_outside', message: 'x' })
    await render()
    await open('Pages')
    await type(byLabel(t('release.fCwd')), '../elsewhere')
    await act(async () => el.querySelector<HTMLButtonElement>('[data-release-save]')!.click())
    const last = useUiStore.getState().toasts.at(-1)!
    expect(last.tone).toBe('err')
    expect(last.text).toBe(t('errors.release_cwd_outside'))
  })
})

describe('凭据录入 / 替换 / 移除', () => {
  it('录入：密文框，存入后框清空、状态变「已存入」，值不留在界面上', async () => {
    const secret = 'otp-secret-991'
    const set = vi.spyOn(api, 'setReleaseCredential').mockResolvedValue({
      ...npm(true), credentials: [{ name: 'NPM_TOKEN', set: true }, { name: 'NPM_OTP', set: true }],
    })
    await render()
    await open('npm')
    const input = cred('NPM_OTP').querySelector<HTMLInputElement>('input')!
    expect(input.type).toBe('password')
    await type(input, secret)
    await act(async () => cred('NPM_OTP').querySelector<HTMLButtonElement>('[data-release-store]')!.click())
    expect(set).toHaveBeenCalledWith('r1', 'NPM_OTP', secret)
    expect(cred('NPM_OTP').querySelector<HTMLInputElement>('input')!.value).toBe('')
    expect(cred('NPM_OTP').textContent).toContain(t('release.credSet'))
    expect(el.innerHTML).not.toContain(secret)
    expect(useUiStore.getState().toasts.at(-1)!.text).toBe(t('release.credStored', { name: 'NPM_OTP' }))
  })

  it('替换：已存入的行再录一次即替换（同一入口）', async () => {
    const set = vi.spyOn(api, 'setReleaseCredential').mockResolvedValue(npm(true))
    await render()
    await open('npm')
    const input = cred('NPM_TOKEN').querySelector<HTMLInputElement>('input')!
    expect(input.placeholder).toBe(t('release.secretReplace'))
    await type(input, 'npm_tok_new')
    await act(async () => cred('NPM_TOKEN').querySelector<HTMLButtonElement>('[data-release-store]')!.click())
    expect(set).toHaveBeenCalledWith('r1', 'NPM_TOKEN', 'npm_tok_new')
  })

  it('移除：A17 danger 确认、首焦「取消」；取消不调用，确认后变「未设置」', async () => {
    const remove = vi.spyOn(api, 'removeReleaseCredential').mockResolvedValue(npm(false))
    await render()
    await open('npm')
    await act(async () => cred('NPM_TOKEN').querySelector<HTMLButtonElement>('[data-release-remove-cred]')!.click())
    const dialog = () => document.querySelector('[role="alertdialog"]')
    expect(dialog()!.textContent).toContain(t('release.removeCredTitle', { name: 'NPM_TOKEN' }))
    expect(document.activeElement?.getAttribute('data-confirm')).toBe('cancel')
    expect(dialog()!.querySelector('[data-confirm="confirm"]')!.getAttribute('data-variant')).toBe('danger')

    await act(async () => document.querySelector<HTMLButtonElement>('[data-confirm="cancel"]')!.click())
    expect(remove).not.toHaveBeenCalled()

    await act(async () => cred('NPM_TOKEN').querySelector<HTMLButtonElement>('[data-release-remove-cred]')!.click())
    await act(async () => document.querySelector<HTMLButtonElement>('[data-confirm="confirm"]')!.click())
    expect(remove).toHaveBeenCalledWith('r1', 'NPM_TOKEN')
    expect(cred('NPM_TOKEN').textContent).toContain(t('release.credMissing'))
    expect(cred('NPM_TOKEN').querySelector('[data-release-remove-cred]')).toBeNull()
  })

  it('删除发布目标：同样先确认（首焦取消），确认后离开列表', async () => {
    const del = vi.spyOn(api, 'removeReleaseTarget').mockResolvedValue(undefined)
    await render()
    await open('Pages')
    await act(async () => el.querySelector<HTMLButtonElement>('[data-release-delete]')!.click())
    expect(document.activeElement?.getAttribute('data-confirm')).toBe('cancel')
    await act(async () => document.querySelector<HTMLButtonElement>('[data-confirm="confirm"]')!.click())
    expect(del).toHaveBeenCalledWith('r2')
    expect(el.querySelectorAll('[role="option"]')).toHaveLength(1)
    expect(el.querySelector('[data-release-detail]')).toBeNull()
  })
})

describe('设置导航', () => {
  it('「发布」是项目级分区：无项目时显示「需要先打开项目」且不发 IPC', async () => {
    await act(async () => root.render(<SettingsPage onBack={() => {}} projectless initialSection="release" />))
    expect(el.querySelector('[data-nav="release"]')!.textContent).toBe(t('settings.nav_release'))
    expect(el.querySelector('[data-empty]')!.textContent).toContain(t('settingsUi.needsProjectTitle'))
    expect(api.releaseTargets).not.toHaveBeenCalled()
  })
})
