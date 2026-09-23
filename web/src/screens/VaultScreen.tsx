import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { IC } from '../icons'
import {
  api,
  ApiError,
  setVaultToken,
  type MediaFile,
  type VaultFolder,
  type VaultScope,
} from '../api/client'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { PhotoViewer } from './PhotoViewer'
import { evictCachedThumbs } from '../pwa'
import { useStickyHeader } from '../hooks/useStickyHeader'
import { useScrollLock } from '../hooks/useScrollLock'

interface Props {
  onClose: () => void
  toast: (msg: string) => void
}

type Phase = 'checking' | 'setup' | 'locked' | 'open'

/** The server's refusals start lowercase; a toast is a sentence. */
function sentence(e: unknown, fallback: string) {
  const m = e instanceof ApiError ? e.message : fallback
  return m.charAt(0).toUpperCase() + m.slice(1)
}

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

  /** Which part of the vault the grid shows: everything, no folder, or one. */
  const [scope, setScope] = useState<VaultScope>('all')
  const [folders, setFolders] = useState<VaultFolder[]>([])
  const [unfiled, setUnfiled] = useState(0)
  const [total, setTotal] = useState(0)
  /** The name prompt, when one is up: making a folder or renaming this one. */
  const [naming, setNaming] = useState<'create' | 'rename' | null>(null)
  const [confirmFolderDelete, setConfirmFolderDelete] = useState(false)
  const [moving, setMoving] = useState(false)
  /** Which photo is open full-screen, as an index into `files`. */
  const [viewing, setViewing] = useState<number | null>(null)

  /** The folder on screen, when the scope is one. */
  const current = typeof scope === 'number' ? (folders.find((f) => f.id === scope) ?? null) : null

  // The newest callbacks, for the one-shot check below to call.
  const onCloseRef = useRef(onClose)
  const toastRef = useRef(toast)
  useEffect(() => {
    onCloseRef.current = onClose
    toastRef.current = toast
  })

  /*
   * Locked or unset — asked once, when the screen opens.
   *
   * It used to re-run whenever `onClose` changed, and App passes a fresh inline
   * one on every render. Every toast re-renders App, so every action here that
   * said anything — hiding, putting back, deleting — set the phase back to
   * `locked` and dropped you on the keypad with the job done behind it. Through
   * refs, the check depends on nothing and runs exactly once.
   */
  useEffect(() => {
    api.vault
      .status()
      .then((s) => setPhase(s.configured ? 'locked' : 'setup'))
      .catch(() => {
        toastRef.current('Could not reach the vault')
        onCloseRef.current()
      })
  }, [])

  /**
   * The grid for `s`, and the folder strip with its counts, in one go.
   *
   * Both after every change, because nearly every change moves a count on the
   * strip as well as the grid — hiding into a folder, moving between two,
   * putting one back. The scope is passed in rather than read from state so a
   * caller that has just changed it loads the new one, not the old.
   */
  const load = useCallback(
    (s: VaultScope) => {
      Promise.all([api.vault.files(s), api.vault.folders()])
        .then(([r, f]) => {
          setFiles(r.files)
          setFolders(f.folders)
          setUnfiled(f.unfiled)
          setTotal(f.total)
        })
        .catch(() => toast('The vault locked itself — enter your PIN again'))
    },
    [toast],
  )

  /** Switch the grid to another folder. A selection belongs to the grid it
      was made in, so it does not follow you. */
  const show = (s: VaultScope) => {
    setScope(s)
    setPicked(null)
    load(s)
  }

  /* Locking on the way out is the point of a short-lived token: leaving the
     screen should not leave the vault readable. */
  const close = useCallback(() => {
    setVaultToken(null)
    onClose()
  }, [onClose])

  const onUnlocked = useCallback(() => {
    setPhase('open')
    load('all')
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
  /**
   * A photo has left the vault — put back or deleted from inside the viewer.
   *
   * The grid drops it here rather than reloading: the viewer is still open on
   * the list either side of it, and swapping the array out from under it would
   * move whatever it paged to next. The folder strip does get refetched, since
   * every count on it just changed.
   */
  const dropHidden = (id: number) => {
    setFiles((fs) => fs.filter((f) => f.id !== id))
    api.vault
      .folders()
      .then((f) => {
        setFolders(f.folders)
        setUnfiled(f.unfiled)
        setTotal(f.total)
      })
      .catch(() => {})
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
        load(scope)
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
        load(scope)
      })
      .catch(() => toast('Could not delete those'))
      .finally(() => setBusy(false))
  }

  /** Make a folder and open it — you made it to put something in it. */
  const createFolder = (name: string) => {
    setBusy(true)
    api.vault
      .createFolder(name)
      .then((r) => {
        setNaming(null)
        toast(`“${r.name}” created`)
        show(r.id)
      })
      .catch((e) => toast(sentence(e, 'Could not make that folder')))
      .finally(() => setBusy(false))
  }

  const renameFolder = (name: string) => {
    if (!current) return
    setBusy(true)
    api.vault
      .renameFolder(current.id, name)
      .then(() => {
        setNaming(null)
        load(scope)
      })
      .catch((e) => toast(sentence(e, 'Could not rename it')))
      .finally(() => setBusy(false))
  }

  /** The folder goes; what was in it stays hidden, in Unfiled. */
  const deleteFolder = () => {
    if (!current) return
    const { id, name } = current
    setBusy(true)
    api.vault
      .deleteFolder(id)
      .then((r) => {
        setConfirmFolderDelete(false)
        toast(
          r.unfiled === 0
            ? `“${name}” deleted`
            : `“${name}” deleted — ${r.unfiled} photo${r.unfiled === 1 ? '' : 's'} moved to Unfiled`,
        )
        show('all')
      })
      .catch(() => toast('Could not delete that folder'))
      .finally(() => setBusy(false))
  }

  /** File the selection into a folder, making it first if it was named. */
  const moveTo = async (dest: { folder_id: number | null } | { name: string }) => {
    if (chosen.length === 0) return
    setBusy(true)
    try {
      let folderId: number | null
      let label: string
      if ('name' in dest) {
        const made = await api.vault.createFolder(dest.name)
        folderId = made.id
        label = made.name
      } else {
        folderId = dest.folder_id
        label =
          folderId === null ? 'Unfiled' : (folders.find((f) => f.id === folderId)?.name ?? 'folder')
      }
      const r = await api.vault.move(
        chosen.map((f) => f.id),
        folderId,
      )
      toast(`${r.moved} moved to ${label}`)
      setMoving(false)
      setPicked(null)
      load(scope)
    } catch (e) {
      toast(sentence(e, 'Could not move those'))
    } finally {
      setBusy(false)
    }
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
        count={total}
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

      <FolderStrip
        folders={folders}
        scope={scope}
        total={total}
        unfiled={unfiled}
        onShow={show}
        onNew={() => setNaming('create')}
      />

      {/* The folder you are in, and the two things you can do to it. Only for a
          real folder — All and Unfiled are views, not things you can rename. */}
      {current && (
        <div className="vf-bar">
          <span className="vf-bar-name">{current.name}</span>
          <button className="pill-btn" onClick={() => setNaming('rename')} disabled={!!picked}>
            Rename
          </button>
          <button
            className="pill-btn vf-danger"
            onClick={() => setConfirmFolderDelete(true)}
            disabled={!!picked}
          >
            Delete folder
          </button>
        </div>
      )}

      {files.length === 0 ? (
        <div className="al-empty" style={{ marginTop: 16 }}>
          {total === 0
            ? 'Nothing hidden yet. Tap + to add photos — they leave your Gallery, your albums, your search and your stats, and only come back here.'
            : current
              ? `Nothing in ${current.name} yet. Tap + to hide photos straight into it, or select some in another folder and move them here.`
              : scope === 'unfiled'
                ? 'Everything hidden is in a folder.'
                : 'Nothing here.'}
        </div>
      ) : (
        <div className="gallery-grid">
          {files.map((f, i) => {
            const on = picked?.has(f.id) ?? false
            return (
              <button
                key={f.id}
                className={`g-cell g-pick ${on ? 'on' : ''}`}
                onClick={() => {
                  if (!picked) {
                    setViewing(i)
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
          <button className="select-act" disabled={busy} onClick={() => setMoving(true)}>
            <IC.folder />
            <span>Move</span>
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

      {/* Was a new browser tab holding a blob URL, which is the one way out of
          this screen that leaves the photo sitting in the app switcher next to
          Safari's other tabs. Same viewer as the Gallery, pinch and page and
          all, and it never leaves the vault. */}
      {viewing !== null && files[viewing] && (
        <PhotoViewer
          files={files}
          index={viewing}
          onIndex={setViewing}
          onClose={() => setViewing(null)}
          mode="vault"
          onRemoved={dropHidden}
          toast={toast}
        />
      )}

      {adding && (
        <AddSheet
          folder={current}
          onClose={() => setAdding(false)}
          onDone={(n) => {
            setAdding(false)
            const what = n === 1 ? '1 photo hidden' : `${n} photos hidden`
            toast(current ? `${what} in ${current.name}` : what)
            load(scope)
          }}
          toast={toast}
        />
      )}

      {moving && (
        <MoveSheet
          folders={folders}
          scope={scope}
          count={chosen.length}
          busy={busy}
          onPick={(d) => void moveTo(d)}
          onClose={() => setMoving(false)}
        />
      )}

      {naming && (
        <NameDialog
          title={naming === 'create' ? 'New folder' : 'Rename folder'}
          initial={naming === 'rename' ? (current?.name ?? '') : ''}
          confirmLabel={naming === 'create' ? 'Create' : 'Rename'}
          busyLabel={naming === 'create' ? 'Creating…' : 'Renaming…'}
          busy={busy}
          onSubmit={naming === 'create' ? createFolder : renameFolder}
          onCancel={() => setNaming(null)}
        />
      )}

      {confirmFolderDelete && current && (
        <ConfirmDialog
          title="Delete this folder?"
          subject={current.name}
          body={
            current.file_count === 0
              ? 'The folder is empty, so nothing else changes.'
              : `Only the folder goes. Its ${current.file_count} photo${current.file_count === 1 ? ' stays' : 's stay'} hidden and ${current.file_count === 1 ? 'moves' : 'move'} to Unfiled.`
          }
          confirmLabel="Delete folder"
          danger
          busy={busy}
          onConfirm={deleteFolder}
          onCancel={() => setConfirmFolderDelete(false)}
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
  folder,
  onClose,
  onDone,
  toast,
}: {
  /** The folder on screen, which is where these land. Null for Unfiled/All. */
  folder: VaultFolder | null
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
      .add(ids, folder?.id)
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
      <div
        className="viewer-sheet share-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={folder ? `Add to ${folder.name}` : 'Add to the vault'}
      >
        <div className="viewer-sheet-grip" aria-hidden="true" />
        <div className="viewer-sheet-head">
          <span className="vf-sheet-title">{folder ? `Add to ${folder.name}` : 'Add to the vault'}</span>
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

/**
 * The vault's folders, as a row of cards above the grid.
 *
 * A filter rather than a way into a separate screen: the vault has one header,
 * one grid and one selection, and a folder changes which photos are in that
 * grid — so moving between folders is a tap, not a back-and-forth. All and
 * Unfiled are drawn as glyphs rather than covers, which is what sets the two
 * views apart from the folders you made.
 */
function FolderStrip({
  folders,
  scope,
  total,
  unfiled,
  onShow,
  onNew,
}: {
  folders: VaultFolder[]
  scope: VaultScope
  total: number
  unfiled: number
  onShow: (s: VaultScope) => void
  onNew: () => void
}) {
  return (
    <div className="albums-scroll vf-strip" role="tablist" aria-label="Vault folders">
      <button
        className={`al-card vf-card plain ${scope === 'all' ? 'on' : ''}`}
        role="tab"
        aria-selected={scope === 'all'}
        onClick={() => onShow('all')}
      >
        <span className="vf-glyph"><IC.photo /></span>
        <span className="al-label-bar">
          <span className="vf-name">All</span>
          <span className="al-count">{total}</span>
        </span>
      </button>

      {/* Only once there is a folder to be outside of: with none, Unfiled is
          All under another name. */}
      {folders.length > 0 && (
        <button
          className={`al-card vf-card plain ${scope === 'unfiled' ? 'on' : ''}`}
          role="tab"
          aria-selected={scope === 'unfiled'}
          onClick={() => onShow('unfiled')}
        >
          <span className="vf-glyph"><IC.imgOff /></span>
          <span className="al-label-bar">
            <span className="vf-name">Unfiled</span>
            <span className="al-count">{unfiled}</span>
          </span>
        </button>
      )}

      {folders.map((f) => (
        <button
          key={f.id}
          className={`al-card vf-card ${f.cover_file_id === null ? 'plain' : ''} ${scope === f.id ? 'on' : ''}`}
          role="tab"
          aria-selected={scope === f.id}
          aria-label={`${f.name}, ${f.file_count} item${f.file_count === 1 ? '' : 's'}`}
          onClick={() => onShow(f.id)}
        >
          {f.cover_file_id !== null ? (
            <img className="al-cover" src={api.vault.thumbUrl(f.cover_file_id)} alt="" loading="lazy" />
          ) : (
            <span className="vf-glyph"><IC.folder /></span>
          )}
          <span className="al-label-bar">
            <span className="vf-name">{f.name}</span>
            <span className="al-count">{f.file_count}</span>
          </span>
        </button>
      ))}

      <button className="al-card vf-card vf-new" onClick={onNew} aria-label="New folder">
        <span className="vf-glyph"><IC.plus /></span>
        <span className="al-label-bar">
          <span className="vf-name">New folder</span>
        </span>
      </button>
    </div>
  )
}

/**
 * Where to move the selection: Unfiled, any folder, or a new one named here.
 *
 * The folder they are already in is shown but disabled rather than left out,
 * so the list reads the same from every folder and the choice you can't make
 * is visibly the one you are in.
 */
function MoveSheet({
  folders,
  scope,
  count,
  busy,
  onPick,
  onClose,
}: {
  folders: VaultFolder[]
  scope: VaultScope
  count: number
  busy: boolean
  onPick: (d: { folder_id: number | null } | { name: string }) => void
  onClose: () => void
}) {
  const [name, setName] = useState('')
  const title = `Move ${count} ${count === 1 ? 'photo' : 'photos'} to…`
  return (
    <div
      className="share-backdrop"
      role="presentation"
      onClick={(e) => { if (e.target === e.currentTarget && !busy) onClose() }}
    >
      <div className="viewer-sheet share-sheet" role="dialog" aria-modal="true" aria-label={title}>
        <div className="viewer-sheet-grip" aria-hidden="true" />
        <div className="viewer-sheet-head"><span>{title}</span></div>
        {/* Pinned above the list, so the way to make a folder can't scroll away
            behind a long one — the same shape as the viewer's album picker. */}
        <div className="viewer-album-new">
          <input
            value={name}
            maxLength={80}
            onChange={(e) => setName(e.target.value)}
            placeholder="New folder name…"
            onKeyDown={(e) => {
              if (e.key === 'Enter' && name.trim() && !busy) onPick({ name: name.trim() })
            }}
          />
          <button
            className="pill-btn"
            disabled={!name.trim() || busy}
            onClick={() => onPick({ name: name.trim() })}
          >
            Create
          </button>
        </div>
        <div className="viewer-album-list viewer-sheet-body">
          <button
            className="viewer-album-row"
            disabled={busy || scope === 'unfiled'}
            onClick={() => onPick({ folder_id: null })}
          >
            <span>Unfiled</span>
            <span className="viewer-album-count">no folder</span>
          </button>
          {folders.map((f) => (
            <button
              key={f.id}
              className="viewer-album-row"
              disabled={busy || scope === f.id}
              onClick={() => onPick({ folder_id: f.id })}
            >
              <span className="vf-row-name">{f.name}</span>
              <span className="viewer-album-count">{scope === f.id ? 'here now' : f.file_count}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}

/**
 * Name a folder — new, or renaming the one on screen.
 *
 * The same box as the Albums tab's New album prompt, so the two read as one
 * pattern; separate from it because that one is written for albums, and its
 * copy says so.
 */
function NameDialog({
  title,
  initial,
  confirmLabel,
  busyLabel,
  busy,
  onSubmit,
  onCancel,
}: {
  title: string
  initial: string
  confirmLabel: string
  busyLabel: string
  busy: boolean
  onSubmit: (name: string) => void
  onCancel: () => void
}) {
  const [name, setName] = useState(initial)
  const ref = useRef<HTMLInputElement>(null)
  useScrollLock()

  useEffect(() => {
    ref.current?.focus()
    ref.current?.select()
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onCancel() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancel])

  const clean = name.trim()
  // Renaming to exactly the current name is a no-op; say so by disabling.
  const ready = clean.length > 0 && clean !== initial && !busy

  return (
    <div
      className="confirm-backdrop at-top"
      role="presentation"
      onClick={(e) => { if (e.target === e.currentTarget && !busy) onCancel() }}
    >
      <div className="confirm-box" role="dialog" aria-modal="true" aria-label={title}>
        <div className="confirm-title">{title}</div>
        <div className="confirm-body">
          Folders only sort what is already hidden. Their names never leave the vault.
        </div>
        <input
          ref={ref}
          className="album-name-input"
          value={name}
          maxLength={80}
          placeholder="Folder name…"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && ready) onSubmit(clean) }}
        />
        <div className="confirm-actions">
          <button className="confirm-btn" onClick={onCancel} disabled={busy}>Cancel</button>
          <button className="confirm-btn primary" disabled={!ready} onClick={() => onSubmit(clean)}>
            {busy ? busyLabel : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  )
}
