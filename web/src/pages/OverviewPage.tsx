import React, { useEffect, useRef, useState } from 'react'
import { Card, Row, Col, Statistic, Tag, Select, Space, Empty } from 'antd'
import { ThunderboltOutlined, ClusterOutlined, ClockCircleOutlined } from '@ant-design/icons'
import ReactECharts from 'echarts-for-react'
import dayjs from 'dayjs'
import { useAppStore } from '../store'
import { nodesApi, clusterApi } from '../services/api'
import { useNavigate } from 'react-router-dom'

interface LiveMetric {
  cpu: number
  gpu: number
  ts: number
}

const MAX_POINTS = 90 // ~3 minutes at 2s sampling
const STALE_MS = 15_000 // drop nodes without a fresh report from the average

const OverviewPage: React.FC = () => {
  const { clusterInfo, nodes, fetchClusterInfo, fetchNodes } = useAppStore()
  const navigate = useNavigate()

  const [liveMetrics, setLiveMetrics] = useState<Record<string, LiveMetric>>({})
  const [selectedNode, setSelectedNode] = useState<string>('all')
  const [series, setSeries] = useState<{ time: string[]; cpu: number[]; gpu: number[] }>({ time: [], cpu: [], gpu: [] })

  // Latest report per node, updated on every WS push (used to compute averages).
  const latestRef = useRef<Record<string, LiveMetric>>({})
  const selectedNodeRef = useRef<string>('all')
  selectedNodeRef.current = selectedNode

  useEffect(() => {
    fetchClusterInfo()
    fetchNodes()
  }, [fetchClusterInfo, fetchNodes])

  // WebSocket for live metrics
  useEffect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const token = localStorage.getItem('token')
    const wsUrl = `${protocol}//${window.location.host}/ws${token ? `?token=${token}` : ''}`

    const ws = new WebSocket(wsUrl)

    ws.onopen = () => {
      ws.send(JSON.stringify({})) // subscribe to all nodes (empty node_id means everything)
    }

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        if (data.type !== 'metrics_update' || !data.node_id || !data.payload) return

        const nodeId: string = data.node_id
        const metric: LiveMetric = {
          cpu: data.payload.cpu_usage_percent ?? 0,
          gpu: data.payload.avg_gpu_utilization ?? 0,
          ts: data.payload.timestamp_ms ?? Date.now(),
        }

        latestRef.current[nodeId] = metric
        setLiveMetrics(prev => ({ ...prev, [nodeId]: metric }))

        // Append one point to the rolling chart, honoring the node selector.
        const now = Date.now()
        const sel = selectedNodeRef.current
        let cpu: number
        let gpu: number
        if (sel === 'all') {
          const entries = Object.values(latestRef.current).filter(m => now - m.ts < STALE_MS)
          if (entries.length === 0) return
          cpu = entries.reduce((s, m) => s + m.cpu, 0) / entries.length
          gpu = entries.reduce((s, m) => s + m.gpu, 0) / entries.length
        } else {
          const m = latestRef.current[sel]
          if (!m) return
          cpu = m.cpu
          gpu = m.gpu
        }

        setSeries(prev => {
          const time = [...prev.time, dayjs(now).format('HH:mm:ss')]
          const cpuArr = [...prev.cpu, Number(cpu.toFixed(1))]
          const gpuArr = [...prev.gpu, Number(gpu.toFixed(1))]
          if (time.length > MAX_POINTS) {
            time.shift()
            cpuArr.shift()
            gpuArr.shift()
          }
          return { time, cpu: cpuArr, gpu: gpuArr }
        })
      } catch {}
    }

    return () => ws.close()
  }, [])

  const onlineCount = nodes.filter(n => n.status === 'online').length
  const totalGpus = nodes.reduce((sum, n) => sum + (n.gpu_count || 0), 0)

  const current = (() => {
    const now = Date.now()
    const sel = selectedNode
    if (sel !== 'all') {
      const m = liveMetrics[sel]
      return m ? { cpu: m.cpu, gpu: m.gpu } : null
    }
    const entries = Object.values(liveMetrics).filter(m => now - m.ts < STALE_MS)
    if (entries.length === 0) return null
    return {
      cpu: entries.reduce((s, m) => s + m.cpu, 0) / entries.length,
      gpu: entries.reduce((s, m) => s + m.gpu, 0) / entries.length,
    }
  })()

  const chartOption = {
    backgroundColor: 'transparent',
    tooltip: {
      trigger: 'axis' as const,
      backgroundColor: '#1a1f2e',
      borderColor: '#2a2f3e',
      textStyle: { color: '#ddd' },
      valueFormatter: (v: any) => `${Number(v).toFixed(1)}%`,
    },
    legend: {
      data: ['CPU', 'GPU'],
      textStyle: { color: '#aaa' },
      top: 0,
    },
    grid: { left: 40, right: 20, bottom: 30, top: 36 },
    xAxis: {
      type: 'category' as const,
      data: series.time,
      boundaryGap: false,
      axisLabel: { color: '#aaa' },
      axisLine: { lineStyle: { color: '#2a2f3e' } },
    },
    yAxis: {
      type: 'value' as const,
      max: 100,
      axisLabel: { color: '#aaa', formatter: '{value}%' },
      splitLine: { lineStyle: { color: '#232838' } },
    },
    series: [
      {
        name: 'CPU',
        type: 'line',
        data: series.cpu,
        smooth: true,
        showSymbol: false,
        lineStyle: { width: 2, color: '#1890ff' },
        itemStyle: { color: '#1890ff' },
        areaStyle: { opacity: 0.15, color: '#1890ff' },
      },
      {
        name: 'GPU',
        type: 'line',
        data: series.gpu,
        smooth: true,
        showSymbol: false,
        lineStyle: { width: 2, color: '#fa8c16' },
        itemStyle: { color: '#fa8c16' },
        areaStyle: { opacity: 0.15, color: '#fa8c16' },
      },
    ],
  }

  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={6}>
          <Card style={{ background: '#1a1f2e', borderColor: '#2a2f3e' }}>
            <Statistic title="Total Nodes" value={nodes.length} prefix={<ClusterOutlined />} />
          </Card>
        </Col>
        <Col span={6}>
          <Card style={{ background: '#1a1f2e', borderColor: '#2a2f3e' }}>
            <Statistic title="Online Nodes" value={onlineCount} valueStyle={{ color: '#52c41a' }} prefix={<ClockCircleOutlined />} />
          </Card>
        </Col>
        <Col span={6}>
          <Card style={{ background: '#1a1f2e', borderColor: '#2a2f3e' }}>
            <Statistic title="Total GPUs" value={totalGpus} prefix={<ThunderboltOutlined />} />
          </Card>
        </Col>
        <Col span={6}>
          <Card style={{ background: '#1a1f2e', borderColor: '#2a2f3e' }}>
            <Statistic title="Running Jobs" value={clusterInfo?.running_jobs || 0} />
          </Card>
        </Col>
      </Row>

      {/* Real-time CPU / GPU utilization chart */}
      <Card
        style={{ background: '#1a1f2e', borderColor: '#2a2f3e', marginTop: 16 }}
        title={
          <Space>
            <span style={{ color: '#fff' }}>Real-time Utilization</span>
            <span style={{ color: '#aaa', fontSize: 13 }}>
              CPU <span style={{ color: '#1890ff' }}>{current ? `${current.cpu.toFixed(1)}%` : '--'}</span>
              {'  ·  '}GPU <span style={{ color: '#fa8c16' }}>{current ? `${current.gpu.toFixed(1)}%` : '--'}</span>
            </span>
          </Space>
        }
        extra={
          <Select
            value={selectedNode}
            onChange={setSelectedNode}
            style={{ width: 200 }}
            options={[
              { value: 'all', label: 'All nodes (avg)' },
              ...nodes.map(n => ({ value: n.node_id, label: n.hostname || n.node_id })),
            ]}
          />
        }
      >
        {series.time.length > 0 ? (
          <ReactECharts option={chartOption} style={{ height: 280 }} notMerge />
        ) : (
          <Empty description="Waiting for live metrics..." style={{ padding: '48px 0' }} />
        )}
      </Card>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        {nodes.map(node => {
          const live = liveMetrics[node.node_id]
          return (
            <Col span={6} key={node.node_id}>
              <Card
                style={{ background: '#1a1f2e', borderColor: '#2a2f3e', cursor: 'pointer' }}
                onClick={() => navigate(`/nodes/${node.node_id}`)}
              >
                <div style={{ marginBottom: 8 }}>
                  <span style={{ color: '#fff', fontWeight: 'bold' }}>{node.hostname || node.node_id}</span>
                  <Tag color={node.status === 'online' ? 'green' : node.status === 'degraded' ? 'orange' : 'red'} style={{ marginLeft: 8 }}>
                    {node.status}
                  </Tag>
                </div>
                <div style={{ fontSize: 12, color: '#aaa', marginBottom: 4 }}>
                  {node.gpu_count} GPU{node.gpu_count > 1 ? 's' : ''}
                  {node.cpu_cores ? ` · ${node.cpu_cores} cores` : ''}
                </div>
                {live ? (
                  <div style={{ fontSize: 13 }}>
                    <span style={{ color: '#1890ff' }}>CPU {live.cpu.toFixed(0)}%</span>
                    <span style={{ color: '#2a2f3e', margin: '0 6px' }}>|</span>
                    <span style={{ color: '#fa8c16' }}>GPU {live.gpu.toFixed(0)}%</span>
                  </div>
                ) : (
                  <div style={{ fontSize: 12, color: '#555' }}>no live data</div>
                )}
              </Card>
            </Col>
          )
        })}
      </Row>
    </div>
  )
}

export default OverviewPage
