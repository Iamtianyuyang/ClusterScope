import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Tag, Space, Collapse, Input as AntInput } from 'antd'
import { PlusOutlined, StopOutlined, PlayCircleOutlined, SyncOutlined } from '@ant-design/icons'
import { jobsApi } from '../services/api'
import { useAppStore } from '../store'
import type { ColumnsType } from 'antd/es/table'

const JobPage: React.FC = () => {
  const { nodes } = useAppStore()
  const [jobs, setJobs] = useState<any[]>([])
  const [createModal, setCreateModal] = useState(false)
  const [logModal, setLogModal] = useState<string | null>(null)
  const [logs, setLogs] = useState<any[]>([])
  const [form] = Form.useForm()
  const [follow, setFollow] = useState(false)

  useEffect(() => {
    refreshJobs()
    const interval = setInterval(refreshJobs, 10000)
    return () => clearInterval(interval)
  }, [])

  const refreshJobs = async () => {
    try {
      const data = await jobsApi.list()
      setJobs(data?.jobs || [])
    } catch {}
  }

  const handleCreate = async (values: any) => {
    await jobsApi.create({
      node_id: values.node_id,
      name: values.name,
      executable: values.executable,
      arguments: values.arguments?.split(' ').filter(Boolean) || [],
      working_directory: values.working_directory || '/',
    })
    setCreateModal(false)
    form.resetFields()
    refreshJobs()
  }

  const handleStop = async (jobId: string) => {
    await jobsApi.stop(jobId)
    refreshJobs()
  }

  const handleViewLogs = async (jobId: string) => {
    setLogModal(jobId)
    try {
      const data = await jobsApi.getLogs(jobId)
      setLogs(data)
    } catch {}
  }

  const statusColor: Record<string, string> = {
    queued: 'blue', starting: 'gold', running: 'green',
    stopping: 'orange', succeeded: 'green', failed: 'red', cancelled: 'gray', lost: 'red',
  }

  const columns: ColumnsType<any> = [
    { title: 'Job ID', dataIndex: 'job_id', key: 'job_id', width: 120 },
    { title: 'Name', dataIndex: 'name', key: 'name' },
    { title: 'Node', dataIndex: 'node_id', key: 'node_id' },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (v: string) => <Tag color={statusColor[v] || 'default'}>{v}</Tag>,
    },
    { title: 'Created By', dataIndex: 'created_by', key: 'created_by' },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => new Date(v).toLocaleString(),
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 200,
      render: (_: any, record: any) => (
        <Space>
          {record.status === 'running' && <Button size="small" icon={<StopOutlined />} onClick={() => handleStop(record.job_id)}>Stop</Button>}
          <Button size="small" icon={<SyncOutlined />} onClick={() => handleViewLogs(record.job_id)}>Logs</Button>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateModal(true)} style={{ marginBottom: 16 }}>
        Create Job
      </Button>
      <Table columns={columns} dataSource={jobs} rowKey="job_id" pagination={{ pageSize: 20 }} size="small" />

      <Modal title="Create Job" open={createModal} onCancel={() => setCreateModal(false)} footer={null} width={600}>
        <Form form={form} onFinish={handleCreate} layout="vertical">
          <Form.Item name="node_id" label="Target Node" rules={[{ required: true }]}>
            <Select placeholder="Select node">
              {nodes.map(n => <Select.Option key={n.node_id} value={n.node_id}>{n.hostname || n.node_id}</Select.Option>)}
            </Select>
          </Form.Item>
          <Form.Item name="name" label="Job Name" rules={[{ required: true }]}>
            <Input placeholder="My training job" />
          </Form.Item>
          <Form.Item name="executable" label="Executable" rules={[{ required: true }]}>
            <Input placeholder="/path/to/binary" />
          </Form.Item>
          <Form.Item name="arguments" label="Arguments">
            <Input placeholder="--epochs 10 --batch-size 32" />
          </Form.Item>
          <Form.Item name="working_directory" label="Working Directory">
            <Input placeholder="/workspace" />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit">Create</Button>
          </Form.Item>
        </Form>
      </Modal>

      <Modal title={`Job Logs: ${logModal}`} open={!!logModal} onCancel={() => { setLogModal(null); setFollow(false) }} footer={null} width={800} destroyOnClose>
        <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
          <Button size="small" icon={<SyncOutlined spin={follow} />} onClick={() => setFollow(!follow)}>
            {follow ? 'Following' : 'Follow'}
          </Button>
        </div>
        <div style={{ background: '#000', padding: 12, maxHeight: 500, overflow: 'auto', fontFamily: 'monospace', fontSize: 12 }}>
          {(logs || []).map((log: any, i: number) => (
            <div key={i} style={{ color: log.is_stderr ? '#ff6b6b' : '#e0e0e0' }}>
              {log.log_data}
            </div>
          ))}
        </div>
      </Modal>
    </div>
  )
}

export default JobPage
