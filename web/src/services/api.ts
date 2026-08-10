import axios from 'axios'
import type { Job, AlertRule } from '../types'

const api = axios.create({
  baseURL: '/api',
})

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('token')
      window.location.href = '/login'
    }
    return Promise.reject(error)
  },
)

export const authApi = {
  login: async (username: string, password: string) => {
    const { data } = await api.post('/login', { username, password })
    localStorage.setItem('token', data.access_token)
    return data
  },
  logout: () => {
    localStorage.removeItem('token')
  },
}

export const nodesApi = {
  list: async () => {
    const { data } = await api.get('/nodes')
    return data
  },
  getStatus: async (nodeId: string) => {
    const { data } = await api.get(`/nodes/${nodeId}`)
    return data
  },
  getMetrics: async (nodeId: string) => {
    const { data } = await api.get(`/nodes/${nodeId}/metrics`)
    return data
  },
}

export const metricsApi = {
  getHistory: async (nodeId: string, startTimeMs: number, endTimeMs: number) => {
    const { data } = await api.get('/metrics/history', {
      params: { node_id: nodeId, start_time_ms: startTimeMs, end_time_ms: endTimeMs },
    })
    return data
  },
}

export const jobsApi = {
  list: async (params?: Record<string, string>) => {
    const { data } = await api.get('/jobs', { params })
    return data
  },
  create: async (job: Omit<Job, 'job_id' | 'status' | 'pid' | 'exit_code' | 'error_message' | 'created_at' | 'started_at' | 'finished_at' | 'created_by'>) => {
    const { data } = await api.post('/jobs', job)
    return data
  },
  get: async (jobId: string) => {
    const { data } = await api.get(`/jobs/${jobId}`)
    return data
  },
  stop: async (jobId: string) => {
    const { data } = await api.delete(`/jobs/${jobId}`)
    return data
  },
  getLogs: async (jobId: string, offset = 0, limit = 100) => {
    const { data } = await api.get(`/jobs/${jobId}/logs`, { params: { offset, limit } })
    return data
  },
}

export const alertsApi = {
  listRules: async () => {
    const { data } = await api.get('/alerts/rules')
    return data
  },
  createRule: async (rule: Partial<AlertRule>) => {
    const { data } = await api.post('/alerts/rules', rule)
    return data
  },
  deleteRule: async (ruleId: string) => {
    await api.delete(`/alerts/rules/${ruleId}`)
  },
  getEvents: async () => {
    const { data } = await api.get('/alerts/events')
    return data
  },
  acknowledge: async (ruleId: string, nodeId: string) => {
    await api.post(`/alerts/rules/${ruleId}/ack`, { node_id: nodeId })
  },
}

export const clusterApi = {
  getInfo: async () => {
    const { data } = await api.get('/cluster/info')
    return data
  },
}

export const usersApi = {
  list: async () => {
    const { data } = await api.get('/users')
    return data
  },
  create: async (user: { username: string; password: string; role: string }) => {
    const { data } = await api.post('/users', user)
    return data
  },
}

export default api
