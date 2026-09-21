import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ToastHost } from './components/ToastHost.tsx'
import { ConfirmHost } from './components/ConfirmDialog.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
    <ToastHost />
    <ConfirmHost />
  </StrictMode>,
)
