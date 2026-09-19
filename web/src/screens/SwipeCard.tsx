import { useState, useRef } from 'react'
import { motion, useMotionValue, useTransform, animate } from 'framer-motion'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'
import { Spinner } from '../components/Spinner'

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1024) return b + ' B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

/** A clip's length as `0:04` / `1:02:09`. */
function fmtDuration(secs: number) {
  const t = Math.round(secs)
  const h = Math.floor(t / 3600)
  const m = Math.floor((t % 3600) / 60)
  const s = String(t % 60).padStart(2, '0')
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`
}

/** Pointer travel under which a release on the card is a tap, not a swipe. */
const TAP_SLOP = 6

/** `created_at` is unix seconds. */
function fmtDate(secs: number) {
  if (!secs) return ''
  return new Date(secs * 1000).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  })
}

interface Props {
  file: MediaFile
  badge: 'blur' | 'dup' | null
  onKeep: () => void
  onDelete: () => void
}

export function SwipeCard({ file, badge, onKeep, onDelete }: Props) {
  const [imgErr, setImgErr] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [fav, setFav] = useState(file.is_favorite)
  const x = useMotionValue(0)
  const rotate = useTransform(x, [-300, 0, 300], [-18, 0, 18])
  const overlayKpOpacity = useTransform(x, [40, 120], [0, 1])
  const overlayDelOpacity = useTransform(x, [-120, -40], [1, 0])
  const isVideo = file.media_type !== 'image'

  /*
   * The clip, played in place with controls of our own.
   *
   * The first version mounted a native-controls <video> when Play was tapped
   * and relied on `autoPlay`. On iOS that is two problems: Safari only lets a
   * clip start *with sound* from a play() called inside the tap itself, so the
   * freshly mounted element sat paused under Safari's own player — and that
   * player's collapse and volume buttons landed on the heart and on our Play.
   *
   * So the element exists from the start, with `preload="none"` so a card you
   * swipe past fetches nothing, and the tap calls play() on it directly. No
   * `controls`: a centred button while stopped, the whole clip as a tap target
   * to pause, and a hairline of progress along the bottom edge.
   */
  const videoRef = useRef<HTMLVideoElement>(null)
  const barRef = useRef<HTMLDivElement>(null)
  /** Idle until the first tap; `loading` until frames actually flow. */
  const [vstate, setVstate] = useState<'idle' | 'loading' | 'playing' | 'paused'>('idle')
  /** Once a frame has played, the video replaces the still for good. */
  const [started, setStarted] = useState(false)
  const [duration, setDuration] = useState<number | null>(null)
  const [videoErr, setVideoErr] = useState(false)

  const togglePlay = () => {
    // Every tap on the card lands here, including the release at the end of a
    // swipe. A swipe has moved the card; a tap has not.
    if (Math.abs(x.get()) > TAP_SLOP) return
    const v = videoRef.current
    if (!v || videoErr) return
    if (v.paused || v.ended) {
      if (v.ended) v.currentTime = 0 // belt and braces for the rewind above
      setVstate('loading')
      // Synchronously inside the tap — this call *is* the user gesture iOS
      // asks for. Refused (a codec this browser can't play) falls back to the
      // still rather than leaving a spinner turning forever.
      v.play().catch(() => setVstate(started ? 'paused' : 'idle'))
    } else {
      v.pause()
    }
  }

  /** Progress written straight to the bar: timeupdate is several times a second,
      and re-rendering the card for each is how swipes start to stutter. */
  const onTime = (e: React.SyntheticEvent<HTMLVideoElement>) => {
    const v = e.currentTarget
    const bar = barRef.current
    if (bar && v.duration > 0) bar.style.transform = `scaleX(${v.currentTime / v.duration})`
  }

  const toggleFav = () => {
    setFav((f) => !f) // optimistic; the server is the source of truth below
    api.favorite(file.id)
      .then((r) => setFav(r.is_favorite))
      .catch(() => setFav(file.is_favorite))
  }

  const THRESHOLD = 110

  const onDragEnd = () => {
    const val = x.get()
    if (val > THRESHOLD) {
      animate(x, window.innerWidth * 1.5, { duration: 0.26 })
      setTimeout(onKeep, 260)
    } else if (val < -THRESHOLD) {
      animate(x, -window.innerWidth * 1.5, { duration: 0.26 })
      setTimeout(onDelete, 260)
    } else {
      animate(x, 0, { type: 'spring', stiffness: 300, damping: 30 })
    }
  }

  return (
    <motion.div
      className="swipe-card"
      style={{ x, rotate }}
      drag="x"
      dragElastic={0.85}
      onDragEnd={onDragEnd}
    >
      {imgErr ? (
        <div className="swipe-card-empty">
          <IC.imgOff />
          <div className="swipe-card-empty-txt">{file.filename}</div>
        </div>
      ) : (
        <div className="swipe-card-img">
          {/* Two layers, on purpose. The thumbnail is already cached from the
              batch request, so it paints instantly and the card is never blank;
              the full preview fades in over it once rendered. Judging sharpness
              off the 480px thumb alone is exactly what made review unusable. */}
          <img className="ph" src={api.thumbUrl(file.id)} draggable={false} alt="" aria-hidden="true" />
          <img
            className={`full ${loaded ? 'in' : ''}`}
            src={api.previewUrl(file.id)}
            onLoad={() => setLoaded(true)}
            onError={() => setImgErr(true)}
            draggable={false}
            alt=""
          />
          {isVideo && !videoErr && (
            <video
              ref={videoRef}
              // Hidden until it has a frame to show, so the sharp still stays
              // up through buffering instead of a black box.
              className={`swipe-card-video ${started ? 'in' : ''}`}
              src={api.rawUrl(file.id)}
              preload="none"
              playsInline
              disablePictureInPicture
              onLoadedMetadata={(e) => setDuration(e.currentTarget.duration)}
              onPlaying={() => {
                setStarted(true)
                setVstate('playing')
              }}
              onWaiting={() => setVstate('loading')}
              // Fires at the end as well, which is what brings the button back
              // for a replay — deliberately no `loop`, or it never would.
              onPause={() => setVstate('paused')}
              // Rewind ourselves the moment it ends. Calling play() on an
              // *ended* element leaves the rewind to the engine, and Safari on
              // iOS answered every replay tap by staying on the last frame.
              // Back at 0, a replay is the same paused-at-the-start play() as
              // the first tap — the case that works — and the card shows its
              // opening frame again rather than freezing on the last.
              onEnded={(e) => {
                e.currentTarget.currentTime = 0
              }}
              onTimeUpdate={onTime}
              onError={() => {
                setVideoErr(true)
                setVstate('idle')
              }}
            />
          )}
        </div>
      )}

      <motion.div className="swipe-overlay kp" style={{ opacity: overlayKpOpacity }}>
        <div className="swipe-overlay-tag">KEEP</div>
      </motion.div>
      <motion.div className="swipe-overlay del" style={{ opacity: overlayDelOpacity }}>
        <div className="swipe-overlay-tag">DELETE</div>
      </motion.div>

      {badge === 'blur' && (
        <div className="badge badge-blur">
          <IC.zap /> BLURRY
        </div>
      )}
      {badge === 'dup' && (
        <div className="badge badge-dup">
          <IC.copy /> DUPLICATE
        </div>
      )}

      {/* The whole clip is the button — play, and tap again to pause — with
          the glyph centred only while it is stopped. Pointer-down is left to
          bubble so a swipe that starts on the clip still swipes the card;
          togglePlay tells the two apart. Below the heart and badges, above
          the picture. */}
      {isVideo && !imgErr && !videoErr && (
        <button
          className={`vid-hit ${vstate}`}
          onClick={togglePlay}
          aria-label={vstate === 'playing' ? 'Pause video' : 'Play video'}
        >
          {vstate !== 'playing' && (
            <span className="vid-center" aria-hidden="true">
              {vstate === 'loading' ? <Spinner /> : <IC.play />}
            </span>
          )}
        </button>
      )}
      {isVideo && started && (
        <div className="vid-progress" aria-hidden="true">
          <div ref={barRef} className="vid-progress-fill" />
        </div>
      )}

      <button
        className={`fav-btn ${fav ? 'on' : ''}`}
        onClick={toggleFav}
        onPointerDown={(e) => e.stopPropagation()}
        aria-pressed={fav}
        aria-label={fav ? 'Remove from favourites' : 'Add to favourites'}
      >
        <IC.heart />
      </button>

      <div className="card-info-bar">
        <div className="card-fname">{file.filename}</div>
        <div className="card-fmeta">
          {fmtDate(file.created_at)} · {fmtBytes(file.size_bytes)}
          {duration !== null && Number.isFinite(duration) && ` · ${fmtDuration(duration)}`}
        </div>
      </div>
    </motion.div>
  )
}
