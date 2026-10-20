import { StrictMode, lazy, Suspense } from 'react'
import { createRoot } from 'react-dom/client'
// 字体本地打包（foundations §2；桌面应用不依赖外网字体）：Inter 400/500/600、JetBrains Mono 400/500
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/500.css'
import './index.css'
import App from './App.tsx'
import { ToastHost } from './components/ToastHost.tsx'
import { ConfirmHost } from './components/ConfirmDialog.tsx'
import i18n from './i18n'
import { syncUiLanguageToCore } from './i18n/syncCore'
import { api } from './api'
import { TooltipProvider } from '@/components/ui/tooltip'
import { installContextMenuGuard } from './contextMenuGuard'

// 组件标本页（ui-redesign 票 01）：仅开发环境，`?specimen` 打开，用来和原型 index#tokens / index#type 截图比对。
// import.meta.env.DEV 在生产构建里是字面量 false，动态导入随分支一起被摇掉。
const Specimen = import.meta.env.DEV && new URLSearchParams(location.search).has('specimen')
  ? lazy(() => import('./specimen/Specimen'))
  : null

syncUiLanguageToCore(i18n, api.setUiLanguage)
// A16b：整个应用拦掉 WebView 自带右键菜单（输入区与 Monaco 例外），见 contextMenuGuard.ts。
installContextMenuGuard()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <TooltipProvider>
      {Specimen ? (
        <Suspense fallback={null}><Specimen /></Suspense>
      ) : (
        <>
          <App />
          <ToastHost />
          <ConfirmHost />
        </>
      )}
    </TooltipProvider>
  </StrictMode>,
)
