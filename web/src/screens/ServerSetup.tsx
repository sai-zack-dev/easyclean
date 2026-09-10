import { useState, type FormEvent } from 'react'
import { IC } from '../icons'
import { useSession } from '../store/session'

interface Props {
  onDone: () => void
}

export function ServerSetup({ onDone }: Props) {
  const { backendUrl, setBackendUrl } = useSession()
  const [url, setUrl] = useState(backendUrl)
  const [testing, setTesting] = useState(false)
  const [error, setError] = useState('')

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setError('')
    setTesting(true)
    const trimmed = url.trim().replace(/\/$/, '')
    try {
      const res = await fetch(`${trimmed}/api/health`, { signal: AbortSignal.timeout(5000) })
      if (!res.ok) throw new Error('Server returned error')
      setBackendUrl(trimmed)
      onDone()
    } catch {
      setError('Could not reach the server. Check the URL and try again.')
    } finally {
      setTesting(false)
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
          <h1 className="auth-title">Connect to EasyClean</h1>
          <p className="auth-sub">Enter your EasyClean server address</p>
        </div>

        <form className="auth-form" onSubmit={submit}>
          <div className="auth-field">
            <label className="auth-label" htmlFor="auth-server">
              Server address
            </label>
            <input
              id="auth-server"
              className="auth-input"
              type="url"
              placeholder="http://192.168.1.x:8080"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              required
            />
          </div>
          {error && (
            <div className="auth-error" role="alert">
              {error}
            </div>
          )}
          <button className="auth-btn" type="submit" disabled={testing}>
            {testing ? 'Connecting…' : 'Connect'}
          </button>
        </form>

        <p className="auth-foot">The server must be reachable on this network.</p>
      </div>
    </div>
  )
}
