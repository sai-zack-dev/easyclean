import { useState, useEffect } from 'react'
import { IC } from '../icons'
import { api, type TimelineMonth, type StreakStats } from '../api/client'
import { ProgressRing } from '../components/ProgressRing'
import { Spinner } from '../components/Spinner'
import { useSession } from '../store/session'
import { useStickyHeader } from '../hooks/useStickyHeader'

interface SwipeTarget {
  mode: string
  month?: string
  label: string
}

interface Props {
  onSwipe: (target: SwipeTarget) => void
  /** Duplicates open their own compare screen rather than the swipe deck. */
  onDuplicates: () => void
  toast: (msg: string) => void
}

export function CleanTab({ onSwipe, onDuplicates, toast }: Props) {
  const { user } = useSession()
  const [timeline, setTimeline] = useState<TimelineMonth[]>([])
  const [streakStats, setStreak] = useState<StreakStats | null>(null)
  const [dupGroups, setDupGroups] = useState(0)
  const [dupCount, setDupCount] = useState(0)
  const [blurCount, setBlurCount] = useState(0)
  const [todayCount, setTodayCount] = useState(0)
  const [recentCount, setRecentCount] = useState(0)
  const [scanning, setScanning] = useState(false)
  const [scanMsg, setScanMsg] = useState('')
  /**
   * Whether finished months stay on the list.
   *
   * Off by default: the checklist is a to-do list, and a month you have already
   * cleaned is done — leaving it there pushed the months that still need work
   * below the fold, which on a well-reviewed library is most of them.
   */
  const [showCleaned, setShowCleaned] = useState(false)

  const days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Today']
  const { ref: screenRef, onScroll } = useStickyHeader()

  const load = () => {
    api.statsTimeline().then(setTimeline).catch(() => {})
    api.statsStreak().then(setStreak).catch(() => {})
    // Every card counts what's still waiting to be reviewed, not every file
    // ever flagged — `stats.blurry` includes ones already swiped. `size: 1`
    // because only the `total` is wanted here, not the photos.
    api.duplicateGroups()
      .then((d) => { setDupGroups(d.total_groups); setDupCount(d.total_files) })
      .catch(() => {})
    api.mediaBatch({ mode: 'blurs', size: 1 })
      .then((d) => setBlurCount(d.total))
      .catch(() => {})
    api.mediaBatch({ mode: 'onthisday', size: 1 })
      .then((d) => setTodayCount(d.total))
      .catch(() => {})
    api.mediaBatch({ mode: 'recents', size: 1 })
      .then((d) => setRecentCount(d.total))
      .catch(() => {})
  }

  useEffect(() => { load() }, [])

  const scan = async () => {
    setScanning(true)
    setScanMsg('Scanning…')
    try {
      await api.scan()
      const es = api.scanStatusStream()
      await new Promise<void>((resolve) => {
        es.addEventListener('progress', (e) => {
          // Payload fields come from ScanProgress in the server's SSE handler.
          const d = JSON.parse((e as MessageEvent).data)
          setScanMsg(`${d.added ?? 0} new · ${d.processed ?? 0} analysed`)
        })
        es.addEventListener('done', () => { es.close(); resolve() })
        es.onerror = () => { es.close(); resolve() }
      })
      load()
      toast('Scan complete!')
    } catch {
      toast('Scan failed')
    } finally {
      setScanning(false)
      setScanMsg('')
    }
  }

  const streak = streakStats?.current_streak ?? 0
  const last7 = streakStats?.last_7_days ?? []

  // A month with nothing left to review. `total > 0` matters: a month whose
  // files have all been filed away reads as 0/0, which is finished, not empty.
  const isCleaned = (t: TimelineMonth) => t.total > 0 && t.reviewed >= t.total
  const cleanedCount = timeline.filter(isCleaned).length
  const visibleMonths = showCleaned ? timeline : timeline.filter((t) => !isCleaned(t))

  return (
    <div className="screen" ref={screenRef} onScroll={onScroll}>
      {/* HEADER */}
      <div className="page-header">
        <div>
          <div className="page-title">EasyClean</div>
          <div className="page-subtitle">Welcome, {user?.username ?? 'User'}</div>
        </div>
        <button className="pill-btn" onClick={scan} disabled={scanning}>
          {scanning ? <Spinner /> : <IC.scan />}
          {scanning ? scanMsg || 'Scanning…' : 'Scan'}
        </button>
      </div>

      {/* STREAK CARD */}
      <div className="streak-card">
        <div className="streak-days">
          {days.map((d, i) => (
            <div key={d} className="streak-day">
              <div className={`s-ring ${i === 6 ? 'today' : (last7[i] ?? 0) > 0 ? 'done' : ''}`}>
                <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </div>
              <div className="streak-day-lbl">{d}</div>
            </div>
          ))}
        </div>
        <div className="streak-meta">
          <div className="streak-badge">
            <IC.fire /> {streak} day{streak !== 1 ? 's' : ''} streak!
          </div>
          <div className="streak-badge">
            <IC.trophy /> {streakStats?.total_swipes ?? 0} swipes
          </div>
        </div>
      </div>

      {/* QUICK CARDS */}
      <div className="quick-grid">
        {/* The badge is the number of photos waiting behind the card, so it
            matches the count the deck then shows. It used to be the day of the
            month, which read as a count and never agreed with one. */}
        <div className="quick-card" onClick={() => onSwipe({ mode: 'onthisday', label: 'On This Day' })}>
          <div className="quick-card-top">
            <div className="quick-card-icon"><IC.calendar /></div>
            {todayCount > 0 && <div className="quick-badge">{todayCount}</div>}
          </div>
          <div>
            <div className="quick-card-title">On This Day</div>
            <div className="quick-card-sub">
              {new Date().toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
              {' · '}
              {todayCount} {todayCount === 1 ? 'photo' : 'photos'}
            </div>
          </div>
        </div>

        <div className="quick-card" onClick={() => onSwipe({ mode: 'recents', label: 'Recents' })}>
          <div className="quick-card-top">
            <div className="quick-card-icon"><IC.clock /></div>
            {recentCount > 0 && <div className="quick-badge">{recentCount}</div>}
          </div>
          <div>
            <div className="quick-card-title">Recents</div>
            <div className="quick-card-sub">This week · {recentCount} files</div>
          </div>
        </div>

        <div className="quick-card" onClick={onDuplicates}>
          <div className="quick-card-top">
            <div className="quick-card-icon"><IC.copy /></div>
            {dupGroups > 0 && <div className="quick-badge">{dupGroups}</div>}
          </div>
          <div>
            <div className="quick-card-title">Duplicates</div>
            <div className="quick-card-sub">
              {dupGroups} {dupGroups === 1 ? 'group' : 'groups'} · {dupCount} files
            </div>
          </div>
        </div>

        <div className="quick-card" onClick={() => onSwipe({ mode: 'blurs', label: 'Blurs' })}>
          <div className="quick-card-top">
            <div className="quick-card-icon"><IC.blur /></div>
            {blurCount > 0 && <div className="quick-badge">{blurCount}</div>}
          </div>
          <div>
            <div className="quick-card-title">Blurs</div>
            <div className="quick-card-sub">{blurCount} files</div>
          </div>
        </div>
      </div>

      {/* TIMELINE CHECKLIST */}
      <div className="section-head">
        <div className="section-label">Timeline Checklist</div>
        {/* Only offered once there is something to reveal — a toggle that does
            nothing is worse than no toggle. */}
        {cleanedCount > 0 && (
          <button
            className="section-toggle"
            onClick={() => setShowCleaned((v) => !v)}
            aria-pressed={showCleaned}
          >
            {showCleaned ? 'Hide cleaned' : `Show cleaned (${cleanedCount})`}
          </button>
        )}
      </div>
      <div className="timeline-list">
        {timeline.length === 0 ? (
          <div className="tl-empty">No media scanned yet — tap Scan</div>
        ) : visibleMonths.length === 0 ? (
          // Every month is done. Worth saying so outright rather than showing
          // an empty list that looks like something failed to load.
          <div className="tl-empty">
            <div className="tl-empty-title">Every month is cleaned</div>
            <div>Nothing left to review. Tap Show cleaned to look back.</div>
          </div>
        ) : (
          visibleMonths.map((t) => (
            <div
              key={t.sort_key}
              className="tl-item"
              onClick={() => onSwipe({ mode: 'timeline', month: t.sort_key, label: t.month })}
            >
              <div className="tl-left">
                <div className="tl-month">{t.month}</div>
                <div className={isCleaned(t) ? 'tl-done' : 'tl-progress'}>
                  {isCleaned(t) ? 'Cleaned' : `${t.reviewed}/${t.total}`}
                </div>
              </div>
              <div className="tl-right">
                <ProgressRing val={t.reviewed} total={t.total} />
              </div>
            </div>
          ))
        )}
      </div>
      <div className="pb16" />
    </div>
  )
}
