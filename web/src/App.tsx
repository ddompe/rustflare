import { useState } from 'react'
import { useStatus } from './api'
import { LiveView } from './components/LiveView'
import { CameraPanel } from './components/CameraPanel'
import { CenteringPanel } from './components/CenteringPanel'
import { MountPanel } from './components/MountPanel'
import { TrackingPanel } from './components/TrackingPanel'
import { CapturePanel } from './components/CapturePanel'
import { DisplayPanel } from './components/DisplayPanel'
import { useDisplay } from './display'
import { StoragePanel } from './components/StoragePanel'
import { OpticsPanel } from './components/OpticsPanel'
import { FilesPanel } from './components/FilesPanel'

export function App() {
  const { status, connected } = useStatus()
  const [anchorPick, setAnchorPick] = useState(false)
  const [display, setDisplay, resetDisplay] = useDisplay()
  const cam = status?.camera

  return (
    <div className="app">
      <header>
        <h1>Rustflare</h1>
        <span className={`dot ${connected ? 'ok' : 'bad'}`} title={connected ? 'connected' : 'reconnecting…'} />
        {cam && (
          <span className="stats">
            {cam.name} · {cam.width}×{cam.height} {cam.bits}-bit · {cam.fps.toFixed(1)} fps · mean {(cam.mean * 100).toFixed(0)}%
            {cam.temperature_c != null && ` · ${cam.temperature_c.toFixed(1)} °C`}
          </span>
        )}
      </header>
      {status?.error && <div className="banner">{status.error}</div>}
      {!connected && !status && <div className="banner">Connecting to the Rustflare server…</div>}
      <main>
        <section className="view">
          <LiveView status={status} display={display} setDisplay={setDisplay} anchorPick={anchorPick} onPicked={() => setAnchorPick(false)} />
        </section>
        <aside>
          <CameraPanel status={status} />
          <OpticsPanel status={status} />
          <CenteringPanel status={status} />
          <DisplayPanel d={display} set={setDisplay} reset={resetDisplay} />
          <MountPanel status={status} />
          <TrackingPanel status={status} anchorPick={anchorPick} setAnchorPick={setAnchorPick} />
          <CapturePanel status={status} />
          <StoragePanel status={status} />
          <FilesPanel status={status} />
        </aside>
      </main>
    </div>
  )
}
