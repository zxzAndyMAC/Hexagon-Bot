// 设置「通用」分区（ui-redesign 票 11；settings#general，components E1）：界面语言、主题、数据去向。
// 主题分段只剩 浅色 / 深色 / 跟随系统（原型三项；foundations §10「night 在设置票下线」）。
// 已存 night 的用户在分段上显示为「深色」（取值相同），store 不改写其存档，点任一项即覆盖。
import { useTranslation } from 'react-i18next'
import i18n, { SUPPORTED, setLang, type Locale } from '../../i18n'
import { useUiStore, type ThemePref } from '../../store'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { Icon, type IconName } from '../Icon'
import { DataBoundary } from '../DataBoundary'
import { Group, NativeSelect, SetHead, SetWrap, SRow } from './kit'

const LANG_NAMES: Record<string, string> = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  en: 'English',
  ja: '日本語',
  es: 'Español',
  pt: 'Português',
  fr: 'Français',
}

const THEMES: { pref: ThemePref; icon: IconName }[] = [
  { pref: 'light', icon: 'sun' },
  { pref: 'dark', icon: 'moon' },
  { pref: 'system', icon: 'computer' },
]

export function GeneralSection() {
  const { t } = useTranslation()
  const themePref = useUiStore((s) => s.themePref)
  const setThemePref = useUiStore((s) => s.setThemePref)
  const shown = themePref === 'night' ? 'dark' : themePref
  return (
    <SetWrap>
      <SetHead title={t('settings.nav_general')} desc={t('settingsUi.generalDesc')} />
      <Group>
        <SRow label={t('settings.language')} hint={t('settingsUi.languageHint')}>
          <NativeSelect className="w-[180px]" aria-label={t('settings.language')} value={i18n.language}
            onChange={(e) => setLang(e.target.value as Locale)}>
            {SUPPORTED.map((l) => <option key={l} value={l}>{LANG_NAMES[l]}</option>)}
          </NativeSelect>
        </SRow>
        <SRow label={t('settings.theme')}>
          <Segmented aria-label={t('settings.theme')} value={shown} onValueChange={(v) => setThemePref(v as ThemePref)}>
            {THEMES.map(({ pref, icon }) => (
              <SegmentedItem key={pref} value={pref}>
                <Icon name={icon} size={12} />{t(`settings.theme_${pref}`)}
              </SegmentedItem>
            ))}
          </Segmented>
        </SRow>
      </Group>
      <DataBoundary />
    </SetWrap>
  )
}
