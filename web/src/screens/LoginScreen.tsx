import { useState, type FormEvent } from 'react'
import { IC } from '../icons'
import { api } from '../api/client'
import { useSession } from '../store/session'
import { Spinner } from '../components/Spinner'

interface Props {
  onLogin: () => void
}

export function LoginScreen({ onLogin }: Props) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
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
      setError(err instanceof Error ? err.message : 'Login failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="auth-screen">
      <div className="auth-logo">
        <div className="auth-logo-icon">
          <IC.broom />
        </div>
        <div className="auth-title">EasyClean</div>
        <div className="auth-sub">Sign in to your account</div>
      </div>

      <form className="auth-form" onSubmit={submit}>
        <input
          className="auth-input"
          type="text"
          placeholder="Username"
          autoCapitalize="none"
          autoComplete="username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
        <input
          className="auth-input"
          type="password"
          placeholder="Passcode"
          autoComplete="current-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
        {error && <div className="auth-error">{error}</div>}
        <button className="auth-btn" type="submit" disabled={loading}>
          {loading ? <Spinner /> : 'Sign In'}
        </button>
      </form>
    </div>
  )
}
