import { useState, type FormEvent } from 'react'
import AuthLayout from '../components/AuthLayout'
import { authRequest, type User } from '../lib/auth'

export default function RegisterPage() {
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (busy) return
    setBusy(true)
    setError('')
    const form = new FormData(event.currentTarget)
    try {
      await authRequest<User>('register', 'POST', {
        email: String(form.get('email') || ''),
        password: String(form.get('password') || ''),
      })
      window.location.assign('/dashboard')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Registration failed')
      setBusy(false)
    }
  }

  return <AuthLayout title="Register">
    <form onSubmit={submit}>
      <div className="form-field">
        <label className="sr-only" htmlFor="register-email">Email</label>
        <input id="register-email" name="email" type="email" placeholder="Email" autoComplete="email" maxLength={254} required />
      </div>
      <div className="form-field">
        <label className="sr-only" htmlFor="register-password">Password</label>
        <input id="register-password" name="password" type="password" placeholder="Password (min. 10 characters)" autoComplete="new-password" minLength={10} maxLength={128} required />
      </div>
      <button className="login-button" type="submit" disabled={busy}>{busy ? 'Creating account…' : 'Register'}</button>
    </form>
    <p className="register-line">Already have an account? <a href="/">Login</a></p>
    {error && <p className="form-notice form-error" role="alert">{error}</p>}
  </AuthLayout>
}
