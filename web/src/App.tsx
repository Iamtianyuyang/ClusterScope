import React, { useEffect } from 'react'
import { Routes, Route, Navigate, useNavigate, useLocation } from 'react-router-dom'
import { Layout, Menu, message } from 'antd'
import {
  DashboardOutlined,
  DesktopOutlined,
  ThunderboltOutlined,
  PlayCircleOutlined,
  BellOutlined,
  UserOutlined,
} from '@ant-design/icons'
import { useAppStore } from './store'
import OverviewPage from './pages/OverviewPage'
import NodeDetailPage from './pages/NodeDetailPage'
import ProcessPage from './pages/ProcessPage'
import JobPage from './pages/JobPage'
import AlertPage from './pages/AlertPage'
import LoginPage from './pages/LoginPage'

const { Header, Content, Sider } = Layout

const App: React.FC = () => {
  const { fetchClusterInfo, fetchNodes } = useAppStore()
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    const token = localStorage.getItem('token')
    if (!token && location.pathname !== '/login') {
      navigate('/login')
    }
  }, [location.pathname, navigate])

  useEffect(() => {
    const token = localStorage.getItem('token')
    if (token) {
      fetchClusterInfo()
      fetchNodes()
    }
  }, [fetchClusterInfo, fetchNodes])

  if (location.pathname === '/login') {
    return <LoginPage />
  }

  const menuItems = [
    { key: '/overview', icon: <DashboardOutlined />, label: 'Cluster Overview' },
    { key: '/nodes', icon: <DesktopOutlined />, label: 'Nodes' },
    { key: '/processes', icon: <ThunderboltOutlined />, label: 'Processes' },
    { key: '/jobs', icon: <PlayCircleOutlined />, label: 'Jobs' },
    { key: '/alerts', icon: <BellOutlined />, label: 'Alerts' },
  ]

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Sider style={{ background: '#1a1f2e' }}>
        <div style={{ padding: '16px', color: '#fff', fontSize: '20px', fontWeight: 'bold', textAlign: 'center' }}>
          ClusterScope
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname === '/overview' ? '/overview' : location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
        />
      </Sider>
      <Layout>
        <Header style={{ background: '#1a1f2e', padding: '0 24px', display: 'flex', alignItems: 'center' }}>
          <h2 style={{ color: '#fff', margin: 0 }}>
            {location.pathname === '/overview' ? 'Cluster Overview' :
             location.pathname.startsWith('/nodes') ? 'Node Details' :
             location.pathname === '/processes' ? 'Processes' :
             location.pathname === '/jobs' ? 'Jobs' : 'Alerts'}
          </h2>
        </Header>
        <Content style={{ margin: '16px', padding: 24, background: '#0f1419', minHeight: 280 }}>
          <Routes>
            <Route path="/overview" element={<OverviewPage />} />
            <Route path="/nodes" element={<OverviewPage />} />
            <Route path="/nodes/:nodeId" element={<NodeDetailPage />} />
            <Route path="/processes" element={<ProcessPage />} />
            <Route path="/jobs" element={<JobPage />} />
            <Route path="/alerts" element={<AlertPage />} />
            <Route path="*" element={<Navigate to="/overview" />} />
          </Routes>
        </Content>
      </Layout>
    </Layout>
  )
}

export default App
