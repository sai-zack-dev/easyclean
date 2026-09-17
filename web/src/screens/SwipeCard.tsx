import { useState, useRef } from 'react'
import { motion, useMotionValue, useTransform, animate } from 'framer-motion'
import { IC } from '../icons'
import { api, type MediaFile } from '../api/client'

function fmtBytes(b: number) {
  if (!b) return '0 B'
  if (b < 1024) return b + ' B'
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB'
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB'
  return (b / 1073741824).toFixed(2) + ' GB'
}

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
  const isVideo = file.media_type !== 'image'
  /**
   * Whether the `<video>` exists yet — one tap, not one per card.
   *
   * A video's card showed its ffmpeg first frame and nothing else, so deciding
   * whether to keep a clip meant deciding from a single still. The button is
   * deliberately the only way in: mounting a player for every card would have
   * the server streaming originals as fast as you can swipe past them.
   *
   * Reset by construction — the deck mounts a card per file with `key={id}`.
   */
  const [mounted, setMounted] = useState(false)
  /**
   * Whether it is *currently* paused, which is a different question.
   *
   * Read off the element's own `play`/`pause` events rather than inferred from
   * the tap that started it: the clip has native controls, so the user can
   * pause it without this button ever being touched — and a Pause icon over a
   * stopped video is a control lying about what it will do.
   */
  const [paused, setPaused] = useState(true)
  const videoRef = useRef<HTMLVideoElement>(null)

  const togglePlay = () => {
    if (!mounted) {
      setMounted(true) // the element autoplays on mount
      return
    }
    const v = videoRef.current
    if (!v) return
    if (v.paused) void v.play()
    else v.pause()
  }

  const toggleFav = () => {
    setFav((f) => !f) // optimistic; the server is the source of truth below
    api.favorite(file.id)
      .then((r) => setFav(r.is_favorite))
      .catch(() => setFav(file.is_favorite))
  }
  const x = useMotionValue(0)
  const rotate = useTransform(x, [-300, 0, 300], [-18, 0, 18])
  const overlayKpOpacity = useTransform(x, [40, 120], [0, 1])
  const overlayDelOpacity = useTransform(x, [-120, -40], [1, 0])

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
      className={`swipe-card ${mounted ? 'playing' : ''}`}
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
          {mounted && (
            <video
              ref={videoRef}
              className="swipe-card-video"
              src={api.rawUrl(file.id)}
              poster={api.thumbUrl(file.id)}
              autoPlay
              loop
              playsInline
              // The card is a drag surface with `touch-action: none`, so a
              // pointer that lands on the video would be swiping the deck
              // rather than scrubbing. Keeping it here is what lets the
              // controls work without a swipe firing underneath them.
              onPointerDown={(e) => e.stopPropagation()}
              onPlay={() => setPaused(false)}
              onPause={() => setPaused(true)}
              // Undecodable in this browser: fall back to the still rather
              // than a black rectangle with a dead control over it.
              onError={() => setMounted(false)}
              controls
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

      {/* Opposite the heart, so the two controls that sit on a photo frame it
          rather than crowd one corner. */}
      {isVideo && !imgErr && (
        <button
          className="play-btn"
          onClick={togglePlay}
          onPointerDown={(e) => e.stopPropagation()}
          aria-label={mounted && !paused ? 'Pause' : 'Play this video'}
        >
          {mounted && !paused ? <IC.pause /> : <IC.play />}
        </button>
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
        <div className="card-fmeta">{fmtDate(file.created_at)} · {fmtBytes(file.size_bytes)}</div>
      </div>
    </motion.div>
  )
}
