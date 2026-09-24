import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ToastHost } from './components/ToastHost.tsx'
import { ConfirmHost } from './components/ConfirmDialog.tsx'
import i18n from './i18n'
import { syncUiLanguageToCore } from './i18n/syncCore'
import { api } from './api'

syncUiLanguageToCore(i18n, api.setUiLanguage)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
    <ToastHost />
    <ConfirmHost />
  </StrictMode>,
)
