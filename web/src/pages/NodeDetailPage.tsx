import React, { useEffect, useState } from 'react'
import { useParams } from 'react-router-dom'
import { Card, Tabs, Table, Tag, Progress, Space, Select, DatePicker, Row, Col } from 'antd'
import ReactECharts from 'echarts-for-react'
import { nodesApi, metricsApi } from '../services/api'
import dayjs from 'dayjs'
import type { ColumnsType } from 'antd/es/table'

const NodeDetailPage: React.FC = () => {
  const { nodeId } = useParams<{ nodeId: string }>()
  const [node, setNode] = useState<any>(null)
  const [metrics, setMetrics] = useState<any>(null)
  const [history, setHistory] = useState<any[]>([])
  const [timeRange, setTimeRange] = useState<string>('15m')

  useEffect(() => {
    if (!nodeId) return
    nodesApi.getMetrics(nodeId).then(setMetrics).catch(() => {})
  }, [nodeId])

  const timeRanges: Record<string, [number, number]> = {
    '15m': [Date.now() - 15 * 60 * 1000, Date.now()],
    '1h': [Date.now() - 1 * 60 * 60 * 1000, Date.now()],
    '6h': [Date.now() - 6 * 60 * 60 * 1000, Date.now()],
    '24h': [Date.now() - 24 * 60 * 60 * 1000, Date.now()],
  }

  useEffect(() => {
    if (!nodeId || !timeRanges[timeRange]) return
    const [start, end] = timeRanges[timeRange]
    metricsApi.getHistory(nodeId, start, end).then(setHistory).catch(() => {})
  }, [nodeId, timeRange])

  const gpuColumns: ColumnsType<any> = [
    { title: 'GPU', dataIndex: 'index', key: 'index', width: 60 },
    { title: 'Name', dataIndex: 'name', key: 'name' },
    {
      title: 'Util',
      key: 'util',
      render: (_: any, record: any) => (
        <Progress percent={Math.round(record.utilization_gpu || 0)} size="small" strokeColor={{ '0%': '#108ee9', '100%': '#f5222d' }} />
      ),
    },
    {
      title: 'Memory',
      key: 'memory',
      render: (_: any, record: any) => `${((record.memory_used_bytes || 0) / 1024 / 1024 / 1024).toFixed(1)} / ${((record.memory_total_bytes || 0) / 1024 / 1024 / 1024).toFixed(1)} GB`,
    },
    {
      title: 'Temp',
      dataIndex: 'temperature_celsius',
      key: 'temp',
      render: (v: number) => <span style={{ color: (v || 0) > 80 ? '#f5222d' : '#52c41a' }}>{v ?? '--'}°C</span>,
    },
    {
      title: 'Power',
      key: 'power',
      render: (_: any, record: any) => `${(record.power_watts ?? 0).toFixed(0)} / ${(record.power_limit_watts ?? 0).toFixed(0)} W`,
    },
  ]

  const processColumns: ColumnsType<any> = [
    { title: 'PID', dataIndex: 'pid', key: 'pid' },
    { title: 'User', dataIndex: 'username', key: 'username' },
    { title: 'Command', dataIndex: 'command', key: 'command' },
    {
      title: 'GPU Mem',
      dataIndex: 'gpu_memory_bytes',
      key: 'gpu_mem',
      render: (v: number) => `${(v / 1024 / 1024).toFixed(0)} MB`,
    },
    {
      title: 'CPU',
      dataIndex: 'cpu_percent',
      key: 'cpu',
      render: (v: number) => `${v.toFixed(1)}%`,
    },
  ]

  const cpuOption = {
    title: { text: 'CPU Usage', left: 'center', textStyle: { color: '#fff', fontSize: 14 } },
    tooltip: { trigger: 'axis' as const },
    grid: { left: 30, right: 30, bottom: 30, top: 50 },
    xAxis: { type: 'category' as const, data: [], axisLabel: { color: '#aaa' } },
    yAxis: { type: 'value' as const, max: 100, axisLabel: { color: '#aaa', formatter: '{value}%' } },
    series: [{ data: [], type: 'line', smooth: true, areaStyle: { opacity: 0.3 }, itemStyle: { color: '#1890ff' } }],
  }

  const gpuOption = {
    title: { text: 'GPU Utilization', left: 'center', textStyle: { color: '#fff', fontSize: 14 } },
    tooltip: { trigger: 'axis' as const },
    grid: { left: 30, right: 30, bottom: 30, top: 50 },
    xAxis: { type: 'category' as const, data: [], axisLabel: { color: '#aaa' } },
    yAxis: { type: 'value' as const, max: 100, axisLabel: { color: '#aaa', formatter: '{value}%' } },
    series: history.map((h: any, i: number) => ({
      name: `GPU ${i}`,
      data: h.gpus?.map((g: any) => g.utilization_gpu || 0) || [],
      type: 'line',
      smooth: true,
    })),
  }

  const tabItems = [
    { key: 'overview', label: 'Overview', children: (
      <div>
        <Row gutter={16}>
          <Col span={12}>
            <Card style={{ background: '#1a1f2e', borderColor: '#2a2f3e' }}>
              <ReactECharts option={cpuOption} style={{ height: 250 }} />
            </Card>
          </Col>
          <Col span={12}>
            <Card style={{ background: '#1a1f2e', borderColor: '#2a2f3e' }}>
              <ReactECharts option={gpuOption} style={{ height: 250 }} />
            </Card>
          </Col>
        </Row>
        <div style={{ marginTop: 16 }}>
          <label style={{ color: '#aaa' }}>Time Range: </label>
          <Select value={timeRange} onChange={setTimeRange} style={{ width: 120 }}>
            <Select.Option value="15m">15 min</Select.Option>
            <Select.Option value="1h">1 hour</Select.Option>
            <Select.Option value="6h">6 hours</Select.Option>
            <Select.Option value="24h">24 hours</Select.Option>
          </Select>
        </div>
      </div>
    )},
    { key: 'gpus', label: 'GPUs', children: (
      <Table columns={gpuColumns} dataSource={metrics?.gpus || []} rowKey="index" pagination={false} size="small" style={{ background: '#1a1f2e' }} />
    )},
    { key: 'processes', label: 'Processes', children: (
      <Table columns={processColumns} dataSource={metrics?.gpu_processes || []} rowKey="pid" pagination={false} size="small" />
    )},
  ]

  return (
    <div>
      <h3 style={{ marginBottom: 16 }}>{nodeId}</h3>
      <Tabs defaultActiveKey="overview" items={tabItems} />
    </div>
  )
}

export default NodeDetailPage
