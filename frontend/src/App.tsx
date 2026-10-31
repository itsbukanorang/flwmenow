import { useEffect, useState } from 'react'
import LoginPage from './pages/LoginPage'
import RegisterPage from './pages/RegisterPage'
import DashboardPage from './pages/DashboardPage'
import './App.css'
import './styles/auth-v1.css'

function getPath() {
  return window.location.pathname.replace(/\/+$/, '') || '/'
}

function App() {
  const [path, setPath] = useState(getPath)

  useEffect(() => {
    const handleRouteChange = () => setPath(getPath())
    window.addEventListener('popstate', handleRouteChange)
    return () => window.removeEventListener('popstate', handleRouteChange)
  }, [])

  if (path === '/register') return <RegisterPage />
  if (path === '/dashboard') return <DashboardPage />
  if (path !== '/') window.location.replace('/')
  return <LoginPage />
}

export default App
