import { useEffect, useState } from 'react'
import { authRequest, type User } from '../lib/auth'

export default function DashboardPage() {
  const [ready, setReady] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let mounted = true
    authRequest<User>('me', 'GET').then(() => {
      if (mounted) setReady(true)
    }).catch(() => {
      if (mounted) window.location.replace('/')
    })
    return () => { mounted = false }
  }, [])

  async function logout() {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      await authRequest<{ ok: boolean }>('logout', 'POST')
      window.location.replace('/')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Logout failed')
      setBusy(false)
    }
  }

  if (!ready) return <main className="dashboard-screen"><p>Loading…</p></main>
  return <main className="dashboard-screen">
    <div className="dashboard-content">
      <h1>success</h1>
      <button type="button" onClick={logout} disabled={busy}>{busy ? 'Logging out…' : 'Logout'}</button>
    </div>
    {error && <p role="alert" className="form-error">{error}</p>}
  </main>
}
