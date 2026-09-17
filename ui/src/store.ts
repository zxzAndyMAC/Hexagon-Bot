import { create } from 'zustand'

interface UiState {
  coreStatus: string
  setCoreStatus: (s: string) => void
}

export const useUiStore = create<UiState>((set) => ({
  coreStatus: '…',
  setCoreStatus: (s) => set({ coreStatus: s }),
}))
