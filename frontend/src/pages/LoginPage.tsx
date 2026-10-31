import { useState, type FormEvent } from 'react'
import AuthLayout from '../components/AuthLayout'
import { authRequest, type User } from '../lib/auth'

export default function LoginPage() {
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (busy) return
    setBusy(true)
    setError('')
    const form = new FormData(event.currentTarget)
    try {
      await authRequest<User>('login', 'POST', {
        email: String(form.get('email') || ''),
        password: String(form.get('password') || ''),
      })
      window.location.assign('/dashboard')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Login failed')
      setBusy(false)
    }
  }

  return <AuthLayout title="Login">
    <form onSubmit={submit}>
      <div className="form-field">
        <label className="sr-only" htmlFor="login-email">Email</label>
        <input id="login-email" name="email" type="email" placeholder="Email" autoComplete="email" maxLength={254} required />
      </div>
      <div className="form-field">
        <label className="sr-only" htmlFor="login-password">Password</label>
        <input id="login-password" name="password" type="password" placeholder="Password" autoComplete="current-password" required />
      </div>
      <button className="login-button" type="submit" disabled={busy}>{busy ? 'Signing in…' : 'Login'}</button>
    </form>
    <p className="register-line">Don&apos;t have an account? <a
      href="/register"
      onClick={(event) => {
        event.preventDefault()
        window.history.pushState({}, '', '/register')
        window.dispatchEvent(new PopStateEvent('popstate'))
      }}
    >
      Register
    </a></p>
    {error && <p className="form-notice form-error" role="alert">{error}</p>}
  </AuthLayout>
}
