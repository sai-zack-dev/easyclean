import { useState, type FormEvent } from 'react'
import { IC } from '../icons'
import { ApiError, api } from '../api/client'
import { useSession } from '../store/session'
import { Spinner } from '../components/Spinner'

interface Props {
  onLogin: () => void
}

/**
 * The server's messages are terse and status-shaped ("invalid credentials",
 * "too many login attempts"), and a rate-limit reply used to surface as raw
 * JSON. Say what happened and what to do about it instead.
 */
function loginError(err: unknown): string {
  if (err instanceof ApiError) {
    if (err.status === 401) return 'Incorrect username or passcode.'
    if (err.status === 429) return 'Too many attempts. Wait 15 minutes and try again.'
    if (err.status >= 500) return 'The server ran into a problem. Try again.'
  }
  // fetch rejects with a TypeError when it cannot reach the host at all.
  if (err instanceof TypeError) return 'Cannot reach the server. Check your connection.'
  return err instanceof Error && err.message ? err.message : 'Login failed. Try again.'
}

export function LoginScreen({ onLogin }: Props) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [reveal, setReveal] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const { setToken, setUser } = useSession()

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const res = await api.login(username.trim(), password)
      setToken(res.access_token)
      setUser({ username: res.user.username, role: res.user.role })
      onLogin()
    } catch (err: unknown) {
      setError(loginError(err))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="auth-screen">
      <div className="auth-glow" aria-hidden="true" />

      <div className="auth-panel">
        <div className="auth-logo">
          <div className="auth-logo-icon">
            <IC.broom />
          </div>
          <h1 className="auth-title">Log in to EasyClean</h1>
          <p className="auth-sub">Sign in to your account</p>
        </div>

        <form className="auth-form" onSubmit={submit}>
          <div className="auth-field">
            <label className="auth-label" htmlFor="auth-username">
              Username
            </label>
            <input
              id="auth-username"
              className="auth-input"
              type="text"
              autoCapitalize="none"
              autoComplete="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
            />
          </div>

          <div className="auth-field">
            <label className="auth-label" htmlFor="auth-passcode">
              Passcode
            </label>
            <div className="auth-input-wrap">
              <input
                id="auth-passcode"
                className="auth-input"
                type={reveal ? 'text' : 'password'}
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
              <button
                className="auth-reveal"
                type="button"
                onClick={() => setReveal((v) => !v)}
                aria-label={reveal ? 'Hide passcode' : 'Show passcode'}
                aria-pressed={reveal}
              >
                {reveal ? <IC.eyeOff /> : <IC.eye />}
              </button>
            </div>
          </div>

          {error && (
            <div className="auth-error" role="alert">
              {error}
            </div>
          )}

          <button className="auth-btn" type="submit" disabled={loading}>
            {loading ? <Spinner /> : 'Log In'}
          </button>
        </form>

        <p className="auth-foot">LAN-only — your photos never leave your network.</p>
      </div>
    </div>
  )
}
