import { useCallback, useEffect, useMemo, useState } from 'react'
import { IC } from '../icons'
import { api, ApiError, setVaultToken, type MediaFile } from '../api/client'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { evictCachedThumbs } from '../pwa'
import { useStickyHeader } from '../hooks/useStickyHeader'

interface Props {
  onClose: () => void
  toast: (msg: string) => void
}

type Phase = 'checking' | 'setup' | 'locked' | 'open'

/**
 * The hidden vault.
 *
 * Nothing in the app links here — you arrive by tapping the profile chip in
 * Settings three times, and that is the whole point: a vault with a button is
 * only a folder. The PIN is its own secret rather than the login passcode,
 * because the only person this hides photos from is someone who can already
 * open EasyClean.
 *
 * Photos are added from *inside* the vault, not from the Gallery. A "Move to
 * vault" action out in the normal UI would announce that there is a vault.
 */
export function VaultScreen({ onClose, toast }: Props) {
  const [phase, setPhase] = useState<Phase>('checking')
  const [files, setFiles] = useState<MediaFile[]>([])
  const [picked, setPicked] = useState<Set<number> | null>(null)
  const [adding, setAdding] = useState(false)
  const [confirmRestore, setConfirmRestore] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [busy, setBusy] = useState(false)
  const { ref: screenRef, onScroll } = useStickyHeader()

  useEffect(() => {
    api.vault
      .status()
      .then((s) => setPhase(s.configured ? 'locked' : 'setup'))
      .catch(() => {
        toast('Could not reach the vault')
        onClose()
      })
  }, [onClose, toast])

  const load = useCallback(() => {
    api.vault
      .files()
      .then((r) => setFiles(r.files))
      .catch(() => toast('The vault locked itself — enter your PIN again'))
  }, [toast])

  /* Locking on the way out is the point of a short-lived token: leaving the
     screen should not leave the vault readable. */
  const close = useCallback(() => {
    setVaultToken(null)
    onClose()
  }, [onClose])

  const onUnlocked = useCallback(() => {
    setPhase('open')
    load()
  }, [load])

  const chosen = useMemo(
    () => (picked ? files.filter((f) => picked.has(f.id)) : []),
    [picked, files],
  )

  /**
   * Show one hidden photo full size.
   *
   * Fetched, then opened as a blob. Navigating to the API URL would have put
   * the vault token in the address bar and left it in history — a trail
   * leading back to the vault, on the screen that exists so there is no trail.
   */
  const openHidden = async (id: number) => {
    try {
      const blob = await api.vault.rawBlob(id)
      const url = URL.createObjectURL(blob)
      const w = window.open(url, '_blank', 'noopener')
      if (!w) {
        URL.revokeObjectURL(url)
        toast('Your browser blocked the new tab')
        return
      }
      window.setTimeout(() => URL.revokeObjectURL(url), 60_000)
    } catch {
      toast('The vault locked itself — enter your PIN again')
    }
  }

  const restore = () => {
    if (chosen.length === 0) return
    setBusy(true)
    api.vault
      .restore(chosen.map((f) => f.id))
      .then((r) => {
        toast(`${r.restored} back in your library`)
        setPicked(null)
        setConfirmRestore(false)
        load()
      })
      .catch(() => toast('Could not put those back'))
      .finally(() => setBusy(false))
  }

  const removeForever = () => {
    if (chosen.length === 0) return
    setBusy(true)
    api.vault
      .remove(chosen.map((f) => f.id))
      .then((r) => {
        toast(r.purged === 1 ? 'Deleted' : `${r.purged} deleted`)
        setPicked(null)
        setConfirmDelete(false)
        load()
      })
      .catch(() => toast('Could not delete those'))
      .finally(() => setBusy(false))
  }

  if (phase === 'checking') {
    return (
      <div className="screen vault-screen">
        <VaultHeader onClose={close} count={null} />
        <div className="al-empty" style={{ marginTop: 40 }}>
          Opening…
        </div>
      </div>
    )
  }

  if (phase === 'setup' || phase === 'locked') {
    return (
      <div className="screen vault-screen">
        <VaultHeader onClose={close} count={null} />
        <PinPad mode={phase} onDone={onUnlocked} toast={toast} />
      </div>
    )
  }

  return (
    // The grid can run long, and the header is sticky now — this is what gives
    // it the hairline once photos pass underneath it.
    <div className="screen vault-screen" ref={screenRef} onScroll={onScroll}>
      <VaultHeader
        onClose={close}
        count={files.length}
        right={
          <div className="swipe-actions">
            {!picked && (
              <button className="icon-btn" onClick={() => setAdding(true)} aria-label="Add photos">
                <IC.plus />
              </button>
            )}
            {/* Icon rather than a "Select" pill: with a text button here the
                right-hand cluster was 173px wide, and a title centred on the
                header cannot clear that and still have room to be read. */}
            {picked ? (
              <button className="icon-btn" onClick={() => setPicked(null)} aria-label="Cancel">
                <IC.close />
              </button>
            ) : (
              <button
                className="icon-btn"
                onClick={() => setPicked(new Set())}
                disabled={!files.length}
                aria-label="Select photos"
              >
                <IC.check />
              </button>
            )}
          </div>
        }
      />

      {files.length === 0 ? (
        <div className="al-empty" style={{ marginTop: 32 }}>
          Nothing hidden yet. Tap + to add photos — they leave your Gallery, your albums, your
          search and your stats, and only come back here.
        </div>
      ) : (
        <div className="gallery-grid">
          {files.map((f) => {
            const on = picked?.has(f.id) ?? false
            return (
              <button
                key={f.id}
                className={`g-cell g-pick ${on ? 'on' : ''}`}
                onClick={() => {
                  if (!picked) {
                    void openHidden(f.id)
                    return
                  }
                  const next = new Set(picked)
                  if (on) next.delete(f.id)
                  else next.add(f.id)
                  setPicked(next)
                }}
              >
                <img src={api.vault.thumbUrl(f.id)} alt="" loading="lazy" />
                {picked && <span className="g-tick">{on && <IC.check />}</span>}
              </button>
            )
          })}
        </div>
      )}

      {picked && chosen.length > 0 && (
        <div className="select-bar">
          <button className="select-act" disabled={busy} onClick={() => setConfirmRestore(true)}>
            <IC.restore />
            <span>Put back</span>
          </button>
          <div className="select-count">
            <b>{chosen.length}</b>
            <span>selected</span>
          </div>
          {/* Deleting from here bins the file like anywhere else — it lands in
              the trash rather than vanishing, so a mis-tap in a screen you had
              to unlock is still recoverable. */}
          <button
            className="select-act danger"
            disabled={busy}
            onClick={() => setConfirmDelete(true)}
          >
            <IC.trash />
            <span>Delete</span>
          </button>
        </div>
      )}

      {adding && (
        <AddSheet
          onClose={() => setAdding(false)}
          onDone={(n) => {
            setAdding(false)
            toast(n === 1 ? '1 photo hidden' : `${n} photos hidden`)
            load()
          }}
          toast={toast}
        />
      )}

      {confirmDelete && (
        <ConfirmDialog
          title="Delete forever?"
          body={`${chosen.length} hidden photo${chosen.length === 1 ? '' : 's'} will be erased from disk. There is no trash for the vault — a binned photo would show up in a screen that needs no PIN, so this cannot be undone.`}
          confirmLabel="Delete forever"
          danger
          busy={busy}
          onConfirm={removeForever}
          onCancel={() => setConfirmDelete(false)}
        />
      )}

      {confirmRestore && (
        <ConfirmDialog
          title="Put these back?"
          body={`${chosen.length} photo${chosen.length === 1 ? '' : 's'} will return to your library, where they were before.`}
          confirmLabel="Put back"
          danger={false}
          busy={busy}
          onConfirm={restore}
          onCancel={() => setConfirmRestore(false)}
        />
      )}
    </div>
  )
}

/**
 * The same header the swipe screens and Duplicates use: back, centred title,
 * actions. On the keypad there is no title at all — naming the screen you have
 * not unlocked yet tells anyone holding the phone what it is.
 */
function VaultHeader({
  onClose,
  count,
  right,
}: {
  onClose: () => void
  count: number | null
  right?: React.ReactNode
}) {
  return (
    <div className="swipe-header">
      <button className="back-btn" onClick={onClose} aria-label="Leave the vault">
        <IC.chevLeft />
      </button>
      <div className="swipe-title-wrap">
        {count !== null && (
          <>
            <div className="swipe-title">Vault</div>
            <div className="swipe-sub">
              {count} hidden item{count === 1 ? '' : 's'}
            </div>
          </>
        )}
      </div>
      {right ?? <span aria-hidden="true" />}
    </div>
  )
}

/**
 * The keypad.
 *
 * `setup` asks twice and compares, because a PIN you mistyped once is a vault
 * you cannot open. `locked` asks once and trades it for a token.
 */
function PinPad({
  mode,
  onDone,
  toast,
}: {
  mode: 'setup' | 'locked'
  onDone: () => void
  toast: (m: string) => void
}) {
  const [pin, setPin] = useState('')
  const [first, setFirst] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [shake, setShake] = useState(false)

  const wrong = useCallback(
    (msg: string) => {
      setShake(true)
      window.setTimeout(() => setShake(false), 420)
      setPin('')
      toast(msg)
    },
    [toast],
  )

  const submit = useCallback(
    (value: string) => {
      setBusy(true)
      if (mode === 'setup') {
        if (first === null) {
          setFirst(value)
          setPin('')
          setBusy(false)
          return
        }
        if (first !== value) {
          setFirst(null)
          setBusy(false)
          wrong('Those did not match — start again')
          return
        }
        api.vault
          .setPin(value)
          .then(() => api.vault.unlock(value))
          .then((r) => {
            setVaultToken(r.vault_token)
            onDone()
          })
          .catch((e) => wrong(e instanceof ApiError ? e.message : 'Could not set that PIN'))
          .finally(() => setBusy(false))
        return
      }
      api.vault
        .unlock(value)
        .then((r) => {
          setVaultToken(r.vault_token)
          onDone()
        })
        .catch(() => wrong('Wrong PIN'))
        .finally(() => setBusy(false))
    },
    [first, mode, onDone, wrong],
  )

  const tap = (d: string) => {
    if (busy) return
    const next = pin + d
    if (next.length > 8) return
    setPin(next)
    if (next.length === 4) submit(next)
  }

  const label =
    mode === 'setup'
      ? first === null
        ? 'Choose a 4-digit PIN'
        : 'Enter it again'
      : 'Enter your PIN'

  return (
    <div className="pin-wrap">
      <div className="pin-icon">
        <IC.mark />
      </div>
      <div className="pin-label">{label}</div>
      <div className="pin-hint">
        {mode === 'setup'
          ? 'Not your login passcode. If you forget this, what is in the vault stays there.'
          : 'Three taps on your profile, any time.'}
      </div>
      <div className={`pin-dots ${shake ? 'shake' : ''}`}>
        {[0, 1, 2, 3].map((i) => (
          <span key={i} className={`pin-dot ${pin.length > i ? 'on' : ''}`} />
        ))}
      </div>
      <div className="pin-pad">
        {['1', '2', '3', '4', '5', '6', '7', '8', '9'].map((d) => (
          <button key={d} className="pin-key" onClick={() => tap(d)} disabled={busy}>
            {d}
          </button>
        ))}
        <span />
        <button className="pin-key" onClick={() => tap('0')} disabled={busy}>
          0
        </button>
        <button
          className="pin-key pin-del"
          onClick={() => setPin(pin.slice(0, -1))}
          disabled={busy || !pin}
          aria-label="Delete"
        >
          <IC.chevLeft />
        </button>
      </div>
    </div>
  )
}

/**
 * Pick photos from the library to hide.
 *
 * Lists the ordinary library, which by definition never includes what is
 * already hidden — so there is nothing to filter out here.
 */
function AddSheet({
  onClose,
  onDone,
  toast,
}: {
  onClose: () => void
  onDone: (n: number) => void
  toast: (m: string) => void
}) {
  const [files, setFiles] = useState<MediaFile[] | null>(null)
  const [picked, setPicked] = useState<Set<number>>(new Set())
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api
      .mediaBatch({ mode: 'library', size: 120, page: 1 })
      .then((r) => setFiles(r.files))
      .catch(() => setFiles([]))
  }, [])

  const hide = () => {
    if (picked.size === 0) return
    const ids = [...picked]
    setBusy(true)
    api.vault
      .add(ids)
      .then((r) => {
        // The server drops every derived copy it owns; this is the one it
        // cannot reach. A thumbnail cached while the photo was still ordinary
        // is keyed on its path, so it would keep being served from this
        // browser — hidden everywhere except the device it was hidden on.
        evictCachedThumbs(ids)
        onDone(r.hidden)
      })
      .catch(() => toast('Could not hide those'))
      .finally(() => setBusy(false))
  }

  return (
    <div className="share-backdrop" role="presentation" onClick={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div className="viewer-sheet share-sheet" role="dialog" aria-modal="true" aria-label="Add to the vault">
        <div className="viewer-sheet-grip" aria-hidden="true" />
        <div className="viewer-sheet-head">
          <span>Add to the vault</span>
          <button className="pill-btn" disabled={busy || picked.size === 0} onClick={hide}>
            {busy ? 'Hiding…' : `Hide ${picked.size || ''}`.trim()}
          </button>
        </div>
        <div className="viewer-sheet-body">
          {files === null ? (
            <div className="viewer-album-empty">Loading…</div>
          ) : files.length === 0 ? (
            <div className="viewer-album-empty">Nothing in your library to hide yet.</div>
          ) : (
            <div className="gallery-grid">
              {files.map((f) => {
                const on = picked.has(f.id)
                return (
                  <button
                    key={f.id}
                    className={`g-cell g-pick ${on ? 'on' : ''}`}
                    onClick={() => {
                      const next = new Set(picked)
                      if (on) next.delete(f.id)
                      else next.add(f.id)
                      setPicked(next)
                    }}
                  >
                    <img src={api.thumbUrl(f.id)} alt="" loading="lazy" />
                    <span className="g-tick">{on && <IC.check />}</span>
                  </button>
                )
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
