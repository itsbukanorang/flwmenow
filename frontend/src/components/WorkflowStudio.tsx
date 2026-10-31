import {
  type FormEvent,
  useEffect,
  useRef,
  useState,
} from 'react'

import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  Maximize2,
  Mic,
  Minus,
  Paperclip,
  Minimize2,
  Plus,
  RotateCcw,
  Send,
  Users,
  X,
} from 'lucide-react'

import type { Company } from '../lib/companies'
import '../styles/executives-v1.css'

type Props = {
  company: Company | null
  onBack: () => void
}

type CeoChatResponse = {
  reply?: string
  error?: string
  model?: string
  cached_tokens?: number
}

type TalkMessage = {
  id: number
  role: 'user' | 'ceo'
  text: string
}

type TalkView = 'normal' | 'maximized' | 'minimized'

type SpeechAlternativeLike = {
  transcript: string
}

type SpeechResultLike = {
  readonly length: number
  readonly [index: number]: SpeechAlternativeLike
}

type SpeechEventLike = {
  readonly results: {
    readonly length: number
    readonly [index: number]: SpeechResultLike
  }
}

type SpeechRecognitionLike = {
  lang: string
  interimResults: boolean
  continuous: boolean
  start: () => void
  stop: () => void
  onresult: ((event: SpeechEventLike) => void) | null
  onend: (() => void) | null
  onerror: (() => void) | null
}

type SpeechRecognitionConstructor =
  new () => SpeechRecognitionLike

type Point = {
  x: number
  y: number
}

type ExecutiveRole = 'CTO' | 'CFO' | 'CMO' | 'COO'

const ROLES: ExecutiveRole[] = ['CTO', 'CFO', 'CMO', 'COO']

const ROLE_NAMES: Record<ExecutiveRole, string> = {
  CTO: 'Technology',
  CFO: 'Finance',
  CMO: 'Marketing',
  COO: 'Operations',
}

type ExecutiveStatus = 'On Duty' | 'Standby' | 'On Leave'

function resolveExecutiveStatus(
  onCanvas: boolean,
  verifiedExecutionRunning: boolean,
): ExecutiveStatus {
  if (!onCanvas) return 'On Leave'
  if (verifiedExecutionRunning) return 'On Duty'
  return 'Standby'
}

function loadRoles(companyId?: number): ExecutiveRole[] {
  if (companyId === undefined) return [...ROLES]

  try {
    const raw = localStorage.getItem(`flwme:executives:${companyId}`)
    if (raw === null) return [...ROLES]

    const value: unknown = JSON.parse(raw)
    if (!Array.isArray(value)) return [...ROLES]

    return ROLES.filter((role) => value.includes(role))
  } catch {
    return [...ROLES]
  }
}

type WorkerNode = Point & {
  id: number
  name: string
  owner: ExecutiveRole
}

type DragState =
  | {
      type: 'pan'
      startX: number
      startY: number
      originX: number
      originY: number
    }
  | {
      type: 'ceo'
      startX: number
      startY: number
      originX: number
      originY: number
    }
  | {
      type: 'executive'
      role: ExecutiveRole
      startX: number
      startY: number
      originX: number
      originY: number
      workersAtStart: Array<{
        id: number
        x: number
        y: number
      }>
    }
  | {
      type: 'worker'
      workerId: number
      startX: number
      startY: number
      originX: number
      originY: number
    }
  | null

const CEO_WIDTH = 178
const CEO_HEIGHT = 68
const WORKER_WIDTH = 188
const WORKER_HEIGHT = 66
const EXEC_HEIGHT = 68

function connectorPath(
  from: Point,
  to: Point,
  sourceWidth = CEO_WIDTH,
  sourceHeight = CEO_HEIGHT,
) {
  const dx = to.x - from.x
  const dy = to.y - from.y

  if (Math.abs(dx) > Math.abs(dy)) {
    const direction = dx >= 0 ? 1 : -1

    const sx =
      from.x +
      direction * sourceWidth / 2

    const sy = from.y

    const tx =
      to.x -
      direction * WORKER_WIDTH / 2

    const ty = to.y

    const bend = Math.max(
      65,
      Math.abs(tx - sx) * 0.45,
    )

    return [
      `M ${sx} ${sy}`,
      `C ${sx + direction * bend} ${sy}`,
      `${tx - direction * bend} ${ty}`,
      `${tx} ${ty}`,
    ].join(' ')
  }

  const direction = dy >= 0 ? 1 : -1

  const sx = from.x
  const sy =
    from.y +
    direction * sourceHeight / 2

  const tx = to.x
  const ty =
    to.y -
    direction * WORKER_HEIGHT / 2

  const bend = Math.max(
    65,
    Math.abs(ty - sy) * 0.45,
  )

  return [
    `M ${sx} ${sy}`,
    `C ${sx} ${sy + direction * bend}`,
    `${tx} ${ty - direction * bend}`,
    `${tx} ${ty}`,
  ].join(' ')
}

export default function WorkflowStudio({
  company,
  onBack,
}: Props) {
  const dragRef = useRef<DragState>(null)
  const canvasRef = useRef<HTMLDivElement>(null)
  const talkTextareaRef = useRef<HTMLTextAreaElement>(null)
  const attachmentInputRef = useRef<HTMLInputElement>(null)
  const talkThreadRef = useRef<HTMLDivElement>(null)
  const talkBoxRef = useRef<HTMLDivElement>(null)
  const speechRef = useRef<SpeechRecognitionLike | null>(null)

  const [zoom, setZoom] = useState(0.8)

  const [pan, setPan] =
    useState<Point>({
      x: 0,
      y: 0,
    })

  const [ceoPosition, setCeoPosition] =
    useState<Point>({
      x: 0,
      y: -140,
    })

  const [workers, setWorkers] =
    useState<WorkerNode[]>([])

  const [teamsOpen, setTeamsOpen] =
    useState(true)

  const [addWorkerOpen, setAddWorkerOpen] =
    useState(false)

  const [workerName, setWorkerName] =
    useState('')

  const [talkText, setTalkText] =
    useState('')

  const [talkNotice, setTalkNotice] =
    useState('')

  const [talkBusy, setTalkBusy] =
    useState(false)

  const [talkMessages, setTalkMessages] =
    useState<TalkMessage[]>([])

  const [talkView, setTalkView] =
    useState<TalkView>('normal')

  const [voiceListening, setVoiceListening] =
    useState(false)

  const [executiveRoles, setExecutiveRoles] =
    useState<ExecutiveRole[]>(() => loadRoles(company?.id))

  const [manageExecutivesOpen, setManageExecutivesOpen] =
    useState(false)

  const [workerOwner, setWorkerOwner] =
    useState<ExecutiveRole | null>(null)

  const [executiveOverrides, setExecutiveOverrides] =
    useState<Partial<Record<ExecutiveRole, Point>>>({})

  useEffect(() => {
    setExecutiveRoles(loadRoles(company?.id))
    setWorkers([])
    setManageExecutivesOpen(false)
    setExecutiveOverrides({})
    setTalkText('')
    setTalkNotice('')
    setTalkBusy(false)
    setTalkMessages([])
    setTalkView('normal')

    try {
      speechRef.current?.stop()
    } catch {
      // recognition may already be stopped
    }

    speechRef.current = null
    setVoiceListening(false)
  }, [company?.id])

  useEffect(() => {
    const thread = talkThreadRef.current
    if (!thread) return

    thread.scrollTop = thread.scrollHeight
  }, [talkMessages, talkBusy])

  const executivePoint = (role: ExecutiveRole): Point => {
    const override = executiveOverrides[role]
    if (override) return override

    const index = ROLES.indexOf(role)
    const compact = window.innerWidth <= 820

    return {
      x: ceoPosition.x +
        (compact
          ? (index % 2 === 0 ? -120 : 120)
          : (index - 1.5) * 215),
      y: ceoPosition.y +
        (compact
          ? 155 + Math.floor(index / 2) * 115
          : 195),
    }
  }

  const toggleExecutive = (role: ExecutiveRole) => {
    if (!company) return

    const removing = executiveRoles.includes(role)

    const next = ROLES.filter((item) =>
      item === role
        ? !removing
        : executiveRoles.includes(item),
    )

    try {
      localStorage.setItem(
        `flwme:executives:${company.id}`,
        JSON.stringify(next),
      )
    } catch {
      setTalkNotice('Unable to save Executive changes.')
      return
    }

    setExecutiveRoles(next)
  }

  const [attachments, setAttachments] =
    useState<File[]>([])

  const changeZoom = (next: number) => {
    setZoom(
      Math.min(
        1.5,
        Math.max(0.55, next),
      ),
    )
  }

  useEffect(() => {
    const canvas = canvasRef.current

    if (!canvas) return

    const handleWheel = (event: WheelEvent) => {
      const target = event.target

      if (
        target instanceof Element &&
        target.closest('.studio-ceo-talk')
      ) {
        return
      }

      event.preventDefault()

      const delta =
        event.deltaY > 0
          ? -0.07
          : 0.07

      setZoom((current) =>
        Math.min(
          1.5,
          Math.max(
            0.55,
            current + delta,
          ),
        ),
      )
    }

    canvas.addEventListener(
      'wheel',
      handleWheel,
      { passive: false },
    )

    return () => {
      canvas.removeEventListener(
        'wheel',
        handleWheel,
      )
    }
  }, [])

  const resizeTalkInput = () => {
    const textarea =
      talkTextareaRef.current

    if (!textarea) return

    textarea.style.height = '0px'

    const nextHeight = Math.min(
      Math.max(
        textarea.scrollHeight,
        28,
      ),
      160,
    )

    textarea.style.height =
      `${nextHeight}px`

    textarea.style.overflowY =
      textarea.scrollHeight > 160
        ? 'auto'
        : 'hidden'
  }

  const resetCanvas = () => {
    setZoom(0.8)
    setPan({ x: 0, y: 0 })
    setCeoPosition({ x: 0, y: -140 })
    setExecutiveOverrides({})
  }

  const openAddWorker = (role: ExecutiveRole) => {
    setWorkerOwner(role)
    setWorkerName('')
    setAddWorkerOpen(true)
  }

  const addWorker = (
    event: FormEvent<HTMLFormElement>,
  ) => {
    event.preventDefault()

    const clean = workerName.trim()

    if (!clean || !workerOwner ||
      !executiveRoles.includes(workerOwner)) return

    const index = workers.filter(
      (worker) => worker.owner === workerOwner,
    ).length

    const parent = executivePoint(workerOwner)
    const columns = [0, 205, -205]

    const x = parent.x + columns[index % 3]

    const y =
      parent.y +
      135 +
      Math.floor(index / 3) * 110

    setWorkers((current) => [
      ...current,
      {
        id: Date.now(),
        name: clean,
        owner: workerOwner,
        x,
        y,
      },
    ])

    setWorkerName('')
    setAddWorkerOpen(false)
  }

  const toggleVoiceInput = () => {
    if (voiceListening) {
      try {
        speechRef.current?.stop()
      } catch {
        // recognition may already be stopped
      }

      return
    }

    const speechWindow = window as Window & {
      SpeechRecognition?: SpeechRecognitionConstructor
      webkitSpeechRecognition?: SpeechRecognitionConstructor
    }

    const Recognition =
      speechWindow.SpeechRecognition ??
      speechWindow.webkitSpeechRecognition

    if (!Recognition) {
      setTalkNotice(
        'Voice input is not supported by this browser.',
      )
      return
    }

    const recognition = new Recognition()
    const existingText = talkText.trimEnd()

    recognition.lang =
      navigator.language || 'en-US'

    recognition.interimResults = true
    recognition.continuous = false

    recognition.onresult = (event) => {
      let transcript = ''

      for (
        let index = 0;
        index < event.results.length;
        index += 1
      ) {
        const result = event.results[index]
        const alternative = result?.[0]

        if (alternative?.transcript) {
          transcript += `${alternative.transcript} `
        }
      }

      const clean = transcript.trim()

      if (!clean) return

      setTalkText(
        existingText
          ? `${existingText} ${clean}`
          : clean,
      )

      setTalkNotice('')

      requestAnimationFrame(
        resizeTalkInput,
      )
    }

    recognition.onerror = () => {
      setVoiceListening(false)
      speechRef.current = null

      setTalkNotice(
        'Voice input could not start. Check microphone permission and try again.',
      )
    }

    recognition.onend = () => {
      setVoiceListening(false)
      speechRef.current = null
    }

    speechRef.current = recognition

    try {
      recognition.start()
      setVoiceListening(true)
      setTalkNotice('')
    } catch {
      speechRef.current = null
      setVoiceListening(false)

      setTalkNotice(
        'Voice input is already active or unavailable.',
      )
    }
  }

  const sendTalk = async () => {
    const message = talkText.trim()

    if (!message || !company || talkBusy) return

    if (attachments.length > 0) {
      setTalkNotice(
        'File runtime is not connected yet. Send a text message for now.',
      )
      return
    }

    setTalkBusy(true)
    setTalkNotice('')

    setTalkMessages((current) => [
      ...current,
      {
        id: Date.now(),
        role: 'user',
        text: message,
      },
    ])

    setTalkText('')
    setAttachments([])

    requestAnimationFrame(
      resizeTalkInput,
    )

    try {
      const response = await fetch('/api/ceo/chat', {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          company_id: company.id,
          message,
        }),
      })

      const data =
        (await response.json()) as CeoChatResponse

      if (!response.ok) {
        throw new Error(
          data.error || 'CEO is unavailable',
        )
      }

      const reply = data.reply?.trim()

      if (!reply) {
        throw new Error(
          'CEO returned an empty response',
        )
      }

      setTalkMessages((current) => [
        ...current,
        {
          id: Date.now() + 1,
          role: 'ceo',
          text: reply,
        },
      ])
    } catch (error) {
      setTalkNotice(
        error instanceof Error
          ? error.message
          : 'CEO is unavailable',
      )
    } finally {
      setTalkBusy(false)
    }
  }

  if (!company) {
    return (
      <section className="studio-company-missing">
        <img
          src="/assets/ceo-icon.png"
          alt=""
          className="studio-missing-icon"
        />

        <h2>Select a company first</h2>

        <p>
          Workflow Studio belongs to a
          specific company.
        </p>

        <button
          type="button"
          onClick={onBack}
        >
          <ArrowLeft size={15} />
          My Companies
        </button>
      </section>
    )
  }

  return (
    <section className="studio-shell">
      <header className="studio-header">
        <div className="studio-header-left">
          <button
            type="button"
            className="studio-back"
            onClick={onBack}
          >
            <ArrowLeft size={16} />
          </button>

          <div className="studio-company-mark">
            {company.name
              .charAt(0)
              .toUpperCase()}
          </div>

          <div>
            <small>WORKFLOW STUDIO</small>
            <h1>{company.name}</h1>
          </div>
        </div>

        <div className="studio-header-actions">
          <button
            type="button"
            onClick={() =>
              changeZoom(zoom - 0.1)
            }
          >
            <Minus size={15} />
          </button>

          <span>
            {Math.round(zoom * 100)}%
          </span>

          <button
            type="button"
            onClick={() =>
              changeZoom(zoom + 0.1)
            }
          >
            <Plus size={15} />
          </button>

          <button
            type="button"
            onClick={resetCanvas}
          >
            <RotateCcw size={15} />
          </button>

          <button
            type="button"
            onClick={() =>
              setTeamsOpen(
                (value) => !value,
              )
            }
          >
            {teamsOpen ? (
              <ChevronRight size={15} />
            ) : (
              <ChevronLeft size={15} />
            )}
          </button>
        </div>
      </header>

      <div
        className={
          teamsOpen
            ? 'studio-workspace'
            : 'studio-workspace teams-hidden'
        }
      >
        <div
          ref={canvasRef}
          className="studio-canvas"
          onDragOver={(event) => {
            if (event.dataTransfer.types.includes('application/x-flwme-executive')) {
              event.preventDefault()
              event.dataTransfer.dropEffect = 'move'
            }
          }}
          onDrop={(event) => {
            const name = event.dataTransfer.getData('application/x-flwme-executive')
            if (!ROLES.includes(name as ExecutiveRole)) return
            event.preventDefault()
            event.stopPropagation()
            const role = name as ExecutiveRole
            const rect = event.currentTarget.getBoundingClientRect()
            const position: Point = {
              x: (event.clientX - rect.left - rect.width / 2 - pan.x) / zoom,
              y: (event.clientY - rect.top - rect.height / 2 - pan.y) / zoom,
            }
            const before = executivePoint(role)
            const dx = position.x - before.x
            const dy = position.y - before.y
            setExecutiveOverrides((current) => ({ ...current, [role]: position }))
            setWorkers((current) => current.map((worker) =>
              worker.owner === role
                ? { ...worker, x: worker.x + dx, y: worker.y + dy }
                : worker,
            ))
            if (!executiveRoles.includes(role)) toggleExecutive(role)
          }}
          onPointerDown={(event) => {
            const target =
              event.target as HTMLElement

            if (
              target.closest(
                'button,input,textarea,.studio-node-card',
              )
            ) {
              return
            }

            event.preventDefault()

            event.currentTarget
              .setPointerCapture(
                event.pointerId,
              )

            dragRef.current = {
              type: 'pan',
              startX: event.clientX,
              startY: event.clientY,
              originX: pan.x,
              originY: pan.y,
            }
          }}
          onPointerMove={(event) => {
            const drag =
              dragRef.current

            if (
              !drag ||
              drag.type !== 'pan'
            ) {
              return
            }

            setPan({
              x:
                drag.originX +
                event.clientX -
                drag.startX,
              y:
                drag.originY +
                event.clientY -
                drag.startY,
            })
          }}
          onPointerUp={() => {
            if (
              dragRef.current?.type ===
              'pan'
            ) {
              dragRef.current = null
            }
          }}
          onPointerCancel={() => {
            dragRef.current = null
          }}
        >
          <div className="studio-canvas-label">
            <span>
              Organization Canvas
            </span>

            <small>
              Drag canvas · Drag nodes · Scroll to zoom
            </small>
          </div>

          <div
            className="studio-scene"
            style={{
              transform:
                `translate(${pan.x}px, ${pan.y}px) ` +
                `scale(${zoom})`,
            }}
          >
            <svg
              className="studio-links"
              viewBox="-1000 -700 2000 1400"
              aria-hidden="true"
            >
              {executiveRoles.length > 0 && (
                <path
                  d={[
                    `M ${ceoPosition.x} ${ceoPosition.y + CEO_HEIGHT / 2}`,
                    `L ${ceoPosition.x} ${ceoPosition.y + 108}`,
                  ].join(' ')}
                  className="studio-link-path"
                  vectorEffect="non-scaling-stroke"
                />
              )}

              {executiveRoles.map((role) => {
                const point = executivePoint(role)
                const forkY = ceoPosition.y + 108

                const targetY =
                  point.y < forkY
                    ? point.y + EXEC_HEIGHT / 2
                    : point.y - EXEC_HEIGHT / 2

                const middleY = (forkY + targetY) / 2

                return (
                  <path
                    key={role}
                    d={[
                      `M ${ceoPosition.x} ${forkY}`,
                      `C ${ceoPosition.x} ${middleY}`,
                      `${point.x} ${middleY}`,
                      `${point.x} ${targetY}`,
                    ].join(' ')}
                    className="studio-link-path"
                    vectorEffect="non-scaling-stroke"
                  />
                )
              })}

              {workers.filter((worker) => executiveRoles.includes(worker.owner)).map((worker) => (
                <path
                  key={worker.id}
                  d={connectorPath(
                    executivePoint(worker.owner),
                    worker,
                    WORKER_WIDTH,
                    WORKER_HEIGHT,
                  )}
                  className="studio-link-path"
                  vectorEffect="non-scaling-stroke"
                />
              ))}
            </svg>

            <div
              className="studio-ceo-node studio-node-card"
              style={{
                left: '50%',
                top: '50%',
                transform:
                  `translate(-50%, -50%) ` +
                  `translate(${ceoPosition.x}px, ${ceoPosition.y}px)`,
              }}
              onPointerDown={(event) => {
                event.preventDefault()
                event.stopPropagation()

                event.currentTarget
                  .setPointerCapture(
                    event.pointerId,
                  )

                dragRef.current = {
                  type: 'ceo',
                  startX:
                    event.clientX,
                  startY:
                    event.clientY,
                  originX:
                    ceoPosition.x,
                  originY:
                    ceoPosition.y,
                }
              }}
              onPointerMove={(event) => {
                const drag =
                  dragRef.current

                if (
                  !drag ||
                  drag.type !== 'ceo'
                ) {
                  return
                }

                setCeoPosition({
                  x:
                    drag.originX +
                    (event.clientX -
                      drag.startX) /
                      zoom,
                  y:
                    drag.originY +
                    (event.clientY -
                      drag.startY) /
                      zoom,
                })
              }}
              onPointerUp={() => {
                if (
                  dragRef.current
                    ?.type === 'ceo'
                ) {
                  dragRef.current = null
                }
              }}
            >
              <div className="studio-node-icon">
                <img
                  src="/assets/ceo-icon.png"
                  alt="CEO"
                  draggable={false}
                />
              </div>

              <div className="studio-node-copy">
                <small>EXECUTIVE</small>
                <strong>CEO</strong>
                <span>
                  Not configured
                </span>
              </div>


            </div>

            {executiveRoles.map((role) => {
              const point = executivePoint(role)

              return (
                <div
                  key={role}
                  className="studio-worker-node studio-node-card studio-exec-node"
                  style={{
                    left: '50%',
                    top: '50%',
                    transform:
                      `translate(-50%, -50%) ` +
                      `translate(${point.x}px, ${point.y}px)`,
                  }}
                  onPointerDown={(event) => {
                    const target = event.target as HTMLElement

                    if (target.closest('button')) return

                    event.preventDefault()
                    event.stopPropagation()
                    event.currentTarget.setPointerCapture(event.pointerId)

                    const point = executivePoint(role)

                    dragRef.current = {
                      type: 'executive',
                      role,
                      startX: event.clientX,
                      startY: event.clientY,
                      originX: point.x,
                      originY: point.y,
                      workersAtStart: workers
                        .filter((worker) => worker.owner === role)
                        .map((worker) => ({
                          id: worker.id,
                          x: worker.x,
                          y: worker.y,
                        })),
                    }
                  }}
                  onPointerMove={(event) => {
                    const drag = dragRef.current

                    if (
                      drag?.type !== 'executive' ||
                      drag.role !== role
                    ) return

                    const dx =
                      (event.clientX - drag.startX) / zoom
                    const dy =
                      (event.clientY - drag.startY) / zoom

                    setExecutiveOverrides((current) => ({
                      ...current,
                      [role]: {
                        x: drag.originX + dx,
                        y: drag.originY + dy,
                      },
                    }))

                    setWorkers((current) =>
                      current.map((worker) => {
                        const original = drag.workersAtStart.find(
                          (item) => item.id === worker.id,
                        )

                        return original
                          ? {
                              ...worker,
                              x: original.x + dx,
                              y: original.y + dy,
                            }
                          : worker
                      }),
                    )
                  }}
                  onPointerUp={() => {
                    if (dragRef.current?.type === 'executive') {
                      dragRef.current = null
                    }
                  }}
                  onPointerCancel={() => {
                    if (dragRef.current?.type === 'executive') {
                      dragRef.current = null
                    }
                  }}
                >
                  <div className="studio-worker-icon studio-exec-icon">
                    <img
                      src={`/assets/${role.toLowerCase()}icon.png`}
                      alt=""
                      draggable={false}
                    />
                  </div>

                  <div className="studio-node-copy">
                    <small>EXECUTIVE</small>
                    <strong>{role}</strong>
                    <span>{ROLE_NAMES[role]}</span>
                  </div>

                  <div className="studio-exec-buttons">
                    <button
                      type="button"
                      title={`Add Worker to ${role}`}
                      aria-label={`Add Worker to ${role}`}
                      onClick={() => openAddWorker(role)}
                    >
                      <Plus size={13} />
                    </button>
                    <button
                      type="button"
                      title={`Remove ${role}`}
                      aria-label={`Remove ${role}`}
                      onClick={() => toggleExecutive(role)}
                    >
                      <X size={12} />
                    </button>
                  </div>
                </div>
              )
            })}

            {workers.filter((worker) => executiveRoles.includes(worker.owner)).map((worker) => (
              <div
                key={worker.id}
                className="studio-worker-node studio-node-card"
                style={{
                  left: '50%',
                  top: '50%',
                  transform:
                    `translate(-50%, -50%) ` +
                    `translate(${worker.x}px, ${worker.y}px)`,
                }}
                onPointerDown={(event) => {
                  event.preventDefault()
                  event.stopPropagation()

                  event.currentTarget
                    .setPointerCapture(
                      event.pointerId,
                    )

                  dragRef.current = {
                    type: 'worker',
                    workerId: worker.id,
                    startX:
                      event.clientX,
                    startY:
                      event.clientY,
                    originX:
                      worker.x,
                    originY:
                      worker.y,
                  }
                }}
                onPointerMove={(event) => {
                  const drag =
                    dragRef.current

                  if (
                    !drag ||
                    drag.type !==
                      'worker' ||
                    drag.workerId !==
                      worker.id
                  ) {
                    return
                  }

                  const x =
                    drag.originX +
                    (event.clientX -
                      drag.startX) /
                      zoom

                  const y =
                    drag.originY +
                    (event.clientY -
                      drag.startY) /
                      zoom

                  setWorkers((current) =>
                    current.map(
                      (item) =>
                        item.id ===
                        worker.id
                          ? {
                              ...item,
                              x,
                              y,
                            }
                          : item,
                    ),
                  )
                }}
                onPointerUp={() => {
                  if (
                    dragRef.current
                      ?.type ===
                      'worker'
                  ) {
                    dragRef.current = null
                  }
                }}
              >
                <div className="studio-worker-icon">
                  <img
                    src="/assets/worker-icon.png"
                    alt="Worker"
                    draggable={false}
                  />
                </div>

                <div className="studio-node-copy">
                  <small>WORKER</small>

                  <strong>
                    {worker.name}
                  </strong>

                  <span>
                    Draft · not saved
                  </span>
                </div>
              </div>
            ))}
          </div>

          <div
            ref={talkBoxRef}
            className={`studio-ceo-talk studio-ceo-talk-${talkView}`}
            onPointerDown={(event) =>
              event.stopPropagation()
            }
            onMouseDown={(event) =>
              event.stopPropagation()
            }
          >
            <div className="studio-talk-heading">
              <div>
                <img
                  src="/assets/ceo-icon.png"
                  alt=""
                  draggable={false}
                />

                <strong>CEO Talk</strong>
              </div>

              <div className="studio-talk-window-controls">
                {talkView === 'minimized' ? (
                  <button
                    type="button"
                    title="Maximize CEO Talk"
                    aria-label="Maximize CEO Talk"
                    onClick={() =>
                      setTalkView('normal')
                    }
                  >
                    <Maximize2 size={15} />
                  </button>
                ) : (
                  <>
                    <button
                      type="button"
                      title="Minimize CEO Talk"
                      aria-label="Minimize CEO Talk"
                      onClick={() =>
                        setTalkView('minimized')
                      }
                    >
                      <Minus size={15} />
                    </button>

                    <button
                      type="button"
                      title={
                        talkView === 'maximized'
                          ? 'Restore CEO Talk'
                          : 'Maximize CEO Talk'
                      }
                      aria-label={
                        talkView === 'maximized'
                          ? 'Restore CEO Talk'
                          : 'Maximize CEO Talk'
                      }
                      onClick={() =>
                        setTalkView(
                          (current) =>
                            current === 'maximized'
                              ? 'normal'
                              : 'maximized',
                        )
                      }
                    >
                      {talkView === 'maximized' ? (
                        <Minimize2 size={15} />
                      ) : (
                        <Maximize2 size={15} />
                      )}
                    </button>
                  </>
                )}
              </div>
            </div>

            <div
              ref={talkThreadRef}
              className="studio-talk-thread"
            >
              {talkMessages.length === 0 &&
                !talkBusy && (
                  <div className="studio-talk-empty">
                    Good Day, What's Our Plan Today?
                  </div>
                )}

              {talkMessages.map((item) => (
                <div
                  key={item.id}
                  className={
                    item.role === 'user'
                      ? 'studio-talk-row studio-talk-row-user'
                      : 'studio-talk-row studio-talk-row-ceo'
                  }
                >
                  <div
                    className={
                      item.role === 'user'
                        ? 'studio-talk-bubble studio-talk-bubble-user'
                        : 'studio-talk-bubble studio-talk-bubble-ceo'
                    }
                  >
                    {item.text}
                  </div>
                </div>
              ))}

              {talkBusy && (
                <div className="studio-talk-row studio-talk-row-ceo">
                  <div className="studio-talk-bubble studio-talk-bubble-ceo studio-talk-bubble-thinking">
                    CEO is thinking…
                  </div>
                </div>
              )}
            </div>

            {attachments.length > 0 && (
              <div className="studio-talk-attachments">
                {attachments.map((file, index) => (
                  <span
                    key={`${file.name}-${index}`}
                  >
                    <Paperclip size={11} />
                    {file.name}

                    <button
                      type="button"
                      onClick={() =>
                        setAttachments(
                          (current) =>
                            current.filter(
                              (_, itemIndex) =>
                                itemIndex !== index,
                            ),
                        )
                      }
                      aria-label={`Remove ${file.name}`}
                    >
                      <X size={10} />
                    </button>
                  </span>
                ))}
              </div>
            )}

            <div className="studio-talk-input">
              <input
                ref={attachmentInputRef}
                type="file"
                multiple
                hidden
                onChange={(event) => {
                  const files =
                    Array.from(
                      event.target.files ?? [],
                    )

                  if (files.length) {
                    setAttachments(
                      (current) => [
                        ...current,
                        ...files,
                      ],
                    )
                  }

                  event.target.value = ''
                }}
              />

              <button
                type="button"
                className="studio-talk-attach"
                onClick={() =>
                  attachmentInputRef.current?.click()
                }
                title="Attach files"
                aria-label="Attach files"
              >
                <Paperclip size={15} />
              </button>

              <button
                type="button"
                className={
                  voiceListening
                    ? 'studio-talk-voice active'
                    : 'studio-talk-voice'
                }
                onClick={toggleVoiceInput}
                title={
                  voiceListening
                    ? 'Stop voice input'
                    : 'Voice to text'
                }
                aria-label="Voice to text"
              >
                <Mic size={15} />
              </button>

              <textarea
                ref={talkTextareaRef}
                rows={1}
                value={talkText}
                onChange={(event) => {
                  setTalkText(
                    event.target.value,
                  )
                  setTalkNotice('')

                  requestAnimationFrame(
                    resizeTalkInput,
                  )
                }}
                placeholder="Type your message here"
                onKeyDown={(event) => {
                  if (
                    event.key === 'Enter' &&
                    !event.shiftKey &&
                    !event.nativeEvent.isComposing
                  ) {
                    event.preventDefault()
                    void sendTalk()
                  }
                }}
                onPointerDown={(event) =>
                  event.stopPropagation()
                }
              />

              <button
                type="button"
                className="studio-talk-send"
                disabled={
                  talkBusy ||
                  !talkText.trim()
                }
                onClick={sendTalk}
              >
                <Send size={15} />
              </button>
            </div>

            {talkNotice && (
              <p className="studio-talk-notice">
                {talkNotice}
              </p>
            )}
          </div>

          <div className="studio-canvas-controls">
            <button
              type="button"
              onClick={resetCanvas}
            >
              <Maximize2 size={15} />
            </button>
          </div>

          {manageExecutivesOpen && (
            <div className="studio-worker-modal-backdrop"
              onMouseDown={(event) => {
                if (event.target === event.currentTarget) {
                  setManageExecutivesOpen(false)
                }
              }}
            >
              <div className="studio-worker-modal">
                <div className="studio-worker-modal-head">
                  <div>
                    <small>ORGANIZATION</small>
                    <strong>Manage Executives</strong>
                  </div>
                  <button type="button"
                    onClick={() => setManageExecutivesOpen(false)}
                  >
                    <X size={14} />
                  </button>
                </div>

                <p className="studio-exec-description">
                  CEO is required. Other Executives are optional.
                </p>

                <div className="studio-exec-options">
                  {ROLES.map((role) => (
                    <button
                      type="button"
                      key={role}
                      onClick={() => toggleExecutive(role)}
                    >
                      <strong>{role}</strong>
                      <span>{ROLE_NAMES[role]}</span>
                      <em>
                        {executiveRoles.includes(role)
                          ? 'Remove'
                          : '+ Restore'}
                      </em>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}

          {addWorkerOpen && (
            <div
              className="studio-worker-modal-backdrop"
              onMouseDown={(event) => {
                if (
                  event.target ===
                  event.currentTarget
                ) {
                  setAddWorkerOpen(false)
                }
              }}
            >
              <form
                className="studio-worker-modal"
                onSubmit={addWorker}
              >
                <div className="studio-worker-modal-head">
                  <div>
                    <small>
                      WORKFORCE
                    </small>

                    <strong>
                      Add Worker to {workerOwner}
                    </strong>
                  </div>

                  <button
                    type="button"
                    onClick={() =>
                      setAddWorkerOpen(false)
                    }
                  >
                    <X size={14} />
                  </button>
                </div>

                <label>
                  Worker Name
                </label>

                <input
                  autoFocus
                  value={workerName}
                  onChange={(event) =>
                    setWorkerName(
                      event.target.value,
                    )
                  }
                  placeholder="e.g. Research Worker"
                  maxLength={80}
                />

                <div className="studio-worker-modal-actions">
                  <button
                    type="button"
                    onClick={() =>
                      setAddWorkerOpen(false)
                    }
                  >
                    Cancel
                  </button>

                  <button
                    type="submit"
                    className="primary"
                    disabled={
                      !workerName.trim()
                    }
                  >
                    Add Worker
                  </button>
                </div>
              </form>
            </div>
          )}
        </div>

        {teamsOpen && (
          <aside className="studio-teams">
            <div className="studio-teams-header">
              <div>
                <Users size={16} />
                <strong>Teams</strong>
              </div>

              <span>
                {1 + ROLES.length + workers.length}
              </span>
            </div>

            <div className="studio-team-section">
              <div className="studio-team-title">
                <small>EXECUTIVE</small>
                <span>1</span>
              </div>

              <div className="studio-team-card">
                <span className="studio-team-avatar image">
                  <img
                    src="/assets/ceo-icon.png"
                    alt="CEO"
                    draggable={false}
                  />
                </span>

                <div>
                  <strong>CEO</strong>

                  <span>
                    Not configured
                  </span>
                </div>


              </div>
            </div>

            <div className="studio-team-section">
              <div className="studio-team-title">
                <small>EXECUTIVE TEAM</small>
                <span>{ROLES.length}</span>
              </div>

              {ROLES.map((role) => {
                const onCanvas = executiveRoles.includes(role)
                // No Rust/Modal task receipts yet: On Duty must NOT be invented.
                const status = resolveExecutiveStatus(onCanvas, false)

                return (
                <div
                  className="studio-team-card studio-exec-catalog-card"
                  key={role}
                  draggable
                  title={`Drag ${role} to canvas`}
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = 'move'
                    event.dataTransfer.setData(
                      'application/x-flwme-executive', role,
                    )
                    event.dataTransfer.setData('text/plain', role)
                  }}
                >
                  <span className="studio-team-avatar">
                    <img
                      src={`/assets/${role.toLowerCase()}icon.png`}
                      alt=""
                      draggable={false}
                    />
                  </span>
                  <div>
                    <strong>{role}</strong>
                    <span>{ROLE_NAMES[role]}</span>
                  </div>
                  <span
                    className={`studio-exec-catalog-status ${
                      status === 'On Duty'
                        ? 'is-on-duty'
                        : status === 'Standby'
                          ? 'is-standby'
                          : 'is-on-leave'
                    }`}
                    aria-label={`${role}: ${status}`}
                  >
                    {status}
                  </span>
                </div>
                )
              })}
            </div>

            <div className="studio-team-section workers">
              <div className="studio-team-title">
                <small>WORKERS</small>

                <span>
                  {workers.length}
                </span>
              </div>

              {workers.length === 0 ? (
                <div className="studio-team-empty compact">
                  <img
                    src="/assets/worker-icon.png"
                    alt=""
                    className="studio-team-empty-worker-icon"
                  />

                  <strong>
                    No workers yet
                  </strong>

                  <p>
                    Add Workers under an
                    Executive using +.
                  </p>
                </div>
              ) : (
                <div className="studio-worker-list">
                  {workers.map(
                    (worker) => (
                      <div
                        key={worker.id}
                        className="studio-worker-row"
                      >
                        <span className="worker-image">
                          <img
                            src="/assets/worker-icon.png"
                            alt=""
                          />
                        </span>

                        <div>
                          <strong>
                            {worker.name}
                          </strong>

                          <small>
                            Draft · not saved
                          </small>
                        </div>

                        <button
                          type="button"
                          onClick={() =>
                            setWorkers(
                              (current) =>
                                current.filter(
                                  (item) =>
                                    item.id !==
                                    worker.id,
                                ),
                            )
                          }
                        >
                          <X size={12} />
                        </button>
                      </div>
                    ),
                  )}
                </div>
              )}
            </div>
          </aside>
        )}
      </div>
    </section>
  )
}
