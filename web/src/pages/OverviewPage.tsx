import React, { useEffect, useState } from 'react'
import { Card, Row, Col, Statistic, Tag, Progress, Space } from 'antd'
import { ThunderboltOutlined, ClusterOutlined, ClockCircleOutlined } from '@ant-design/icons'
import { useAppStore } from '../store'
import { nodesApi, clusterApi } from '../services/api'
import { useNavigate } from 'react-router-dom'

const OverviewPage: React.FC = () => {
  const { clusterInfo, nodes, fetchClusterInfo, fetchNodes } = useAppStore()
  const navigate = useNavigate()
  const [liveMetrics, setLiveMetrics] = useState<Record<string, any>>({})

  useEffect(() => {
    fetchClusterInfo()
    fetchNodes()
  }, [fetchClusterInfo, fetchNodes])

  // WebSocket for live metrics
  useEffect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${window.location.host}/ws`

    const ws = new WebSocket(wsUrl)

    ws.onopen = () => {
      ws.send(JSON.stringify({ node_id: '' }))
    }

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        if (data.node_id) {
          setLiveMetrics(prev => ({ ...prev, [data.node_id]: data }))
        }
      } catch {}
    }

    return () => ws.close()
  }, [])

  const onlineCount = nodes.filter(n => n.status === 'online').length
  const totalGpus = nodes.reduce((sum, n) => sum + (n.gpu_count || 0), 0)

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

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        {nodes.map(node => (
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
              {node.gpu_count > 0 && (
                <div style={{ fontSize: 12, color: '#aaa' }}>
                  {node.gpu_count} GPU{node.gpu_count > 1 ? 's' : ''}
                </div>
              )}
            </Card>
          </Col>
        ))}
      </Row>
    </div>
  )
}

export default OverviewPage
