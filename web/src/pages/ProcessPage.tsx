import React, { useEffect, useState } from 'react'
import { Table, Select, Input, Button, Space, Tag, InputNumber } from 'antd'
import { SearchOutlined, ReloadOutlined } from '@ant-design/icons'
import { nodesApi } from '../services/api'
import dayjs from 'dayjs'
import type { ColumnsType } from 'antd/es/table'

const ProcessPage: React.FC = () => {
  const [nodes, setNodes] = useState<any[]>([])
  const [allProcesses, setAllProcesses] = useState<any[]>([])
  const [filterNode, setFilterNode] = useState<string>('')
  const [filterUser, setFilterUser] = useState<string>('')
  const [searchText, setSearchText] = useState('')
  const [sortBy, setSortBy] = useState<'gpu_mem' | 'runtime'>('runtime')

  useEffect(() => {
    nodesApi.list().then(setNodes).catch(() => {})
  }, [])

  useEffect(() => {
    const fetchAll = async () => {
      const all: any[] = []
      for (const node of nodes) {
        try {
          const metrics = await nodesApi.getMetrics(node.node_id)
          for (const proc of (metrics?.gpu_processes || [])) {
            all.push({ ...proc, node_id: node.node_id, hostname: node.hostname })
          }
        } catch {}
      }
      setAllProcesses(all)
    }
    fetchAll()
    const interval = setInterval(fetchAll, 5000)
    return () => clearInterval(interval)
  }, [nodes])

  const filtered = allProcesses
    .filter(p => !filterNode || p.node_id === filterNode)
    .filter(p => !filterUser || p.username === filterUser)
    .filter(p => !searchText || p.command.toLowerCase().includes(searchText.toLowerCase()))
    .sort((a, b) => {
      if (sortBy === 'gpu_mem') return (b.gpu_memory_bytes || 0) - (a.gpu_memory_bytes || 0)
      return (b.started_at || 0) - (a.started_at || 0)
    })

  const users = [...new Set(allProcesses.map(p => p.username).filter(Boolean))]

  const columns: ColumnsType<any> = [
    { title: 'PID', dataIndex: 'pid', key: 'pid', width: 80 },
    { title: 'User', dataIndex: 'username', key: 'username', width: 100 },
    { title: 'Command', dataIndex: 'command', key: 'command', ellipsis: true },
    { title: 'Node', key: 'node', width: 150, render: (_: any, record: any) => `${record.hostname || record.node_id}` },
    { title: 'GPU', key: 'gpu', width: 80, render: (_: any, record: any) => <Tag>{record.gpu_uuid ? record.gpu_uuid.substring(0, 8) : '-'}</Tag> },
    {
      title: 'GPU Mem',
      key: 'gpu_mem',
      width: 100,
      render: (_: any, record: any) => `${(record.gpu_memory_bytes || 0) / 1024 / 1024} MB`,
    },
    {
      title: 'CPU',
      dataIndex: 'cpu_percent',
      key: 'cpu',
      width: 80,
      render: (v: number) => `${v?.toFixed(1) || 0}%`,
    },
    {
      title: 'Runtime',
      key: 'runtime',
      width: 120,
      render: (_: any, record: any) => {
        if (!record.started_at) return '-'
        const seconds = Math.floor((Date.now() / 1000) - record.started_at / 1000)
        const h = Math.floor(seconds / 3600)
        const m = Math.floor((seconds % 3600) / 60)
        return h > 0 ? `${h}h ${m}m` : `${m}m`
      },
    },
  ]

  return (
    <div>
      <div style={{ marginBottom: 16, display: 'flex', gap: 12, alignItems: 'center' }}>
        <Select value={filterNode} onChange={setFilterNode} placeholder="Filter by node" allowClear style={{ width: 200 }}>
          {nodes.map(n => <Select.Option key={n.node_id} value={n.node_id}>{n.hostname || n.node_id}</Select.Option>)}
        </Select>
        <Select value={filterUser} onChange={setFilterUser} placeholder="Filter by user" allowClear style={{ width: 150 }}>
          {users.map(u => <Select.Option key={u} value={u}>{u}</Select.Option>)}
        </Select>
        <Input.Search value={searchText} onChange={e => setSearchText(e.target.value)} placeholder="Search commands" style={{ width: 300 }} />
        <Button icon={<ReloadOutlined />} onClick={() => window.location.reload()}>Refresh</Button>
      </div>
      <Table columns={columns} dataSource={filtered} rowKey="pid" pagination={{ pageSize: 20, showSizeChanger: true }} size="small" />
    </div>
  )
}

export default ProcessPage
