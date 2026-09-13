import { useState } from 'react'
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
