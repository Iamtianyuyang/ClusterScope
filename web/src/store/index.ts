import { create } from 'zustand'
import { clusterApi, nodesApi } from '../services/api'

interface AppState {
  clusterInfo: any
  nodes: any[]
  loading: boolean
  error: string | null
  fetchClusterInfo: () => Promise<void>
  fetchNodes: () => Promise<void>
  wsConnected: boolean
  setWsConnected: (connected: boolean) => void
}

export const useAppStore = create<AppState>((set) => ({
  clusterInfo: null,
  nodes: [],
  loading: false,
  error: null,
  wsConnected: false,

  fetchClusterInfo: async () => {
    try {
      const info = await clusterApi.getInfo()
      set({ clusterInfo: info })
    } catch (e: any) {
      set({ error: e.message })
    }
  },

  fetchNodes: async () => {
    try {
      const nodes = await nodesApi.list()
      set({ nodes })
    } catch (e: any) {
      set({ error: e.message })
    }
  },

  setWsConnected: (connected) => set({ wsConnected: connected }),
}))
