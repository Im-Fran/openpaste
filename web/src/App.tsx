import { useCallback, useEffect, useState } from 'react'

const API: string = import.meta.env.VITE_SERVER ?? ''

type Paste = {
  id: string
  content: string | null
  storage_key: string | null
  filename: string | null
  content_type: string
  size: number
  created_at: number
}

function navigate(path: string) {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
}

const humanSize = (n: number) =>
  n < 1024 ? `${n} B` : n < 1024 ** 2 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 ** 2).toFixed(1)} MB`

export default function App() {
  const [path, setPath] = useState(location.pathname)
  useEffect(() => {
    const on = () => setPath(location.pathname)
    addEventListener('popstate', on)
    return () => removeEventListener('popstate', on)
  }, [])

  const id = path.match(/^\/paste\/([^/]+)/)?.[1]
  return (
    <div className="app">
      <header>
        <a href="/" onClick={(e) => { e.preventDefault(); navigate('/') }}>
          <img src="/icon.svg" alt="" width="28" height="28" />
          <span>openpaste</span>
        </a>
      </header>
      {id ? <View id={id} /> : <New />}
    </div>
  )
}

function New() {
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const upload = useCallback(async (body: BodyInit, filename?: string) => {
    setBusy(true)
    setError(null)
    try {
      const res = await fetch(`${API}/api/pastes`, {
        method: 'POST',
        headers: { Accept: 'application/json', ...(filename ? { 'X-Filename': filename } : {}) },
        body,
      })
      const raw = await res.text()
      if (!res.ok) throw new Error(raw.trim() || res.statusText)
      navigate(`/paste/${JSON.parse(raw).id}`)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }, [])

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const file = e.dataTransfer.files[0]
    if (file) upload(file, file.name)
  }

  return (
    <main onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Paste text or code here — or drop a file anywhere on this page."
        spellCheck={false}
        autoFocus
      />
      {error && <p className="error">{error}</p>}
      <div className="row">
        <button disabled={busy || !text} onClick={() => upload(text)}>
          {busy ? 'uploading…' : 'create paste'}
        </button>
        <label className="file">
          upload a file
          <input
            type="file"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0]
              if (f) upload(f, f.name)
            }}
          />
        </label>
        <code className="hint">echo 'hi' | curl --data-binary @- {location.origin}</code>
      </div>
    </main>
  )
}

function View({ id }: { id: string }) {
  const [paste, setPaste] = useState<Paste | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    setPaste(null)
    setError(null)
    fetch(`${API}/api/pastes/${id}`)
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`paste "${id}" not found`))))
      .then(setPaste)
      .catch((e) => setError(e.message))
  }, [id])

  if (error) return <main><p className="error">{error}</p></main>
  if (!paste) return <main><p className="muted">loading…</p></main>

  const base = `${API}/paste/${paste.id}`
  return (
    <main>
      <div className="meta">
        <span>{paste.filename ?? paste.id}</span>
        <span className="muted">{paste.content_type} · {humanSize(paste.size)}</span>
      </div>
      {paste.content !== null ? (
        <pre>{paste.content}</pre>
      ) : (
        <div className="binary">
          <p>Binary file — nothing to display.</p>
          <a className="button" href={`${base}/download`}>download</a>
        </div>
      )}
      <div className="row">
        {paste.content !== null && (
          <button
            onClick={() => {
              navigator.clipboard.writeText(paste.content!)
              setCopied(true)
              setTimeout(() => setCopied(false), 1500)
            }}
          >
            {copied ? 'copied!' : 'copy'}
          </button>
        )}
        <a className="button" href={`${base}/raw`}>raw</a>
        <a className="button" href={`${base}/download`}>download</a>
        <code className="hint">{location.origin}/paste/{paste.id}</code>
      </div>
    </main>
  )
}
