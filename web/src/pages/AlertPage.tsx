import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, InputNumber, Select, Tag, Space, Popconfirm } from 'antd'
import { PlusOutlined, CheckOutlined } from '@ant-design/icons'
import { alertsApi } from '../services/api'
import type { ColumnsType } from 'antd/es/table'

const AlertPage: React.FC = () => {
  const [rules, setRules] = useState<any[]>([])
  const [events, setEvents] = useState<any[]>([])
  const [createModal, setCreateModal] = useState(false)
  const [form] = Form.useForm()

  useEffect(() => {
    loadRules()
    loadEvents()
  }, [])

  const loadRules = async () => {
    try { const data = await alertsApi.listRules(); setRules(data) } catch {}
  }

  const loadEvents = async () => {
    try { const data = await alertsApi.getEvents(); setEvents(data) } catch {}
  }

  const handleCreate = async (values: any) => {
    await alertsApi.createRule(values)
    setCreateModal(false)
    form.resetFields()
    loadRules()
  }

  const handleDelete = async (ruleId: string) => {
    await alertsApi.deleteRule(ruleId)
    loadRules()
  }

  const handleAck = async (ruleId: string, nodeId: string) => {
    await alertsApi.acknowledge(ruleId, nodeId)
    loadEvents()
  }

  const eventColumns: ColumnsType<any> = [
    { title: 'Rule', dataIndex: 'rule_id', key: 'rule_id' },
    { title: 'Node', dataIndex: 'node_id', key: 'node_id' },
    {
      title: 'State',
      dataIndex: 'new_state',
      key: 'new_state',
      render: (v: string) => <Tag color={v === 'firing' ? 'red' : v === 'pending' ? 'orange' : 'green'}>{v}</Tag>,
    },
    {
      title: 'Value',
      key: 'value',
      render: (_: any, record: any) => `${record.current_value?.toFixed(1)} (threshold: ${record.threshold})`,
    },
    {
      title: 'Time',
      dataIndex: 'timestamp',
      key: 'timestamp',
      render: (v: string) => new Date(v).toLocaleString(),
    },
    {
      title: 'Action',
      key: 'action',
      render: (_: any, record: any) => record.new_state === 'firing' ? (
        <Popconfirm title="Acknowledge?" onConfirm={() => handleAck(record.rule_id, record.node_id)}>
          <Button size="small" icon={<CheckOutlined />}>Ack</Button>
        </Popconfirm>
      ) : null,
    },
  ]

  const ruleColumns: ColumnsType<any> = [
    { title: 'Name', dataIndex: 'name', key: 'name' },
    { title: 'Metric', dataIndex: 'metric', key: 'metric' },
    { title: 'Condition', key: 'condition', render: (_: any, r: any) => `${r.operator} ${r.threshold}` },
    { title: 'Duration', dataIndex: 'duration_seconds', key: 'duration', render: (v: number) => `${v}s` },
    {
      title: 'Severity',
      dataIndex: 'severity',
      key: 'severity',
      render: (v: string) => <Tag color={v === 'critical' ? 'red' : v === 'warning' ? 'orange' : 'blue'}>{v}</Tag>,
    },
    { title: 'Enabled', dataIndex: 'enabled', key: 'enabled', render: (v: boolean) => v ? 'Yes' : 'No' },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: any, record: any) => (
        <Popconfirm title="Delete rule?" onConfirm={() => handleDelete(record.rule_id)}>
          <Button size="small" danger>Delete</Button>
        </Popconfirm>
      ),
    },
  ]

  return (
    <div>
      <h3 style={{ marginBottom: 16 }}>Active Alerts</h3>
      <Table columns={eventColumns} dataSource={events} rowKey="event_id" pagination={false} size="small" style={{ marginBottom: 32 }} />

      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <h3>Alert Rules</h3>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateModal(true)}>Create Rule</Button>
      </div>
      <Table columns={ruleColumns} dataSource={rules} rowKey="rule_id" size="small" />

      <Modal title="Create Alert Rule" open={createModal} onCancel={() => setCreateModal(false)} footer={null}>
        <Form form={form} onFinish={handleCreate} layout="vertical">
          <Form.Item name="name" label="Rule Name" rules={[{ required: true }]}>
            <Input placeholder="GPU temperature too high" />
          </Form.Item>
          <Form.Item name="metric" label="Metric" rules={[{ required: true }]}>
            <Input placeholder="gpu_temperature" />
          </Form.Item>
          <Form.Item name="operator" label="Operator">
            <Select>
              <Select.Option value="gt">Greater than</Select.Option>
              <Select.Option value="gte">Greater or equal</Select.Option>
              <Select.Option value="lt">Less than</Select.Option>
              <Select.Option value="lte">Less or equal</Select.Option>
            </Select>
          </Form.Item>
          <Form.Item name="threshold" label="Threshold" rules={[{ required: true }]}>
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="duration_seconds" label="Duration (seconds)" rules={[{ required: true }]}>
            <InputNumber min={1} style={{ width: '100%' }} defaultValue={30} />
          </Form.Item>
          <Form.Item name="severity" label="Severity">
            <Select>
              <Select.Option value="info">Info</Select.Option>
              <Select.Option value="warning">Warning</Select.Option>
              <Select.Option value="critical">Critical</Select.Option>
            </Select>
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit">Create</Button>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default AlertPage
