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
  Minus,
  Paperclip,
  Plus,
  RotateCcw,
  Send,
  Users,
  X,
} from 'lucide-react'

import type { Company } from '../lib/companies'

type Props = {
  company: Company | null
  onBack: () => void
}

type Point = {
  x: number
  y: number
}

type WorkerNode = Point & {
  id: number
  name: string
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
      type: 'worker'
      workerId: number
      startX: number
      startY: number
      originX: number
      originY: number
    }
  | null

const CEO_WIDTH = 226
const CEO_HEIGHT = 90
const WORKER_WIDTH = 188
const WORKER_HEIGHT = 66

function connectorPath(
  from: Point,
  to: Point,
) {
  const dx = to.x - from.x
  const dy = to.y - from.y

  if (Math.abs(dx) > Math.abs(dy)) {
    const direction = dx >= 0 ? 1 : -1

    const sx =
      from.x +
      direction * CEO_WIDTH / 2

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
    direction * CEO_HEIGHT / 2

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

  const [zoom, setZoom] = useState(1)

  const [pan, setPan] =
    useState<Point>({
      x: 0,
      y: 0,
    })

  const [ceoPosition, setCeoPosition] =
    useState<Point>({
      x: -120,
      y: -90,
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
    setZoom(1)
    setPan({ x: 0, y: 0 })
    setCeoPosition({
      x: -120,
      y: -90,
    })
  }

  const openAddWorker = () => {
    setWorkerName('')
    setAddWorkerOpen(true)
  }

  const addWorker = (
    event: FormEvent<HTMLFormElement>,
  ) => {
    event.preventDefault()

    const clean = workerName.trim()

    if (!clean) return

    const index = workers.length

    const columns = [0, 230, -230]

    const x =
      ceoPosition.x +
      columns[index % 3]

    const y =
      ceoPosition.y +
      190 +
      Math.floor(index / 3) * 120

    setWorkers((current) => [
      ...current,
      {
        id: Date.now(),
        name: clean,
        x,
        y,
      },
    ])

    setWorkerName('')
    setAddWorkerOpen(false)
  }

  const draftTalk = () => {
    if (!talkText.trim()) return

    setTalkNotice(
      'CEO runtime is not connected yet. Message kept as a local draft.',
    )
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
              {workers.map((worker) => (
                <path
                  key={worker.id}
                  d={connectorPath(
                    ceoPosition,
                    worker,
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

              <button
                type="button"
                className="studio-node-add"
                onPointerDown={(event) =>
                  event.stopPropagation()
                }
                onClick={openAddWorker}
                title="Add Worker"
              >
                <Plus size={15} />
              </button>
            </div>

            {workers.map((worker) => (
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

          <div className="studio-ceo-talk">
            <div className="studio-talk-heading">
              <div>
                <img
                  src="/assets/ceo-icon.png"
                  alt=""
                  draggable={false}
                />

                <strong>CEO Talk</strong>
              </div>

              <span>Draft mode</span>
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
                placeholder="What's your plan today, boss?"
                onPointerDown={(event) =>
                  event.stopPropagation()
                }
              />

              <button
                type="button"
                className="studio-talk-send"
                disabled={
                  !talkText.trim() &&
                  attachments.length === 0
                }
                onClick={draftTalk}
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
                      Add Worker
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
                {1 + workers.length}
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

                <button
                  type="button"
                  className="studio-team-add"
                  onClick={openAddWorker}
                >
                  <Plus size={13} />
                </button>
              </div>
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
                    Add a worker from the CEO
                    + button.
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
