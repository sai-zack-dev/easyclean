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
      <div className="auth-logo">
        <div className="auth-logo-icon">
          <IC.broom />
        </div>
        <div className="auth-title">EasyClean</div>
        <div className="auth-sub">Enter your EasyClean server address</div>
      </div>

      <form className="auth-form" onSubmit={submit}>
        <input
          className="auth-input"
          type="url"
          placeholder="http://192.168.1.x:8080"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          required
        />
        {error && <div className="auth-error">{error}</div>}
        <button className="auth-btn" type="submit" disabled={testing}>
          {testing ? 'Connecting…' : 'Connect'}
        </button>
      </form>
    </div>
  )
}
