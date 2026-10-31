import type { ReactNode } from 'react'

const beams = [
  [18, 46, 4, 1], [23, 30, 2, .55], [26, 46, 3, .65],
  [29, 27, 2, .5], [34, 40, 3, .4], [39, 54, 5, .4],
  [43, 30, 2, .35], [57, 34, 3, .35], [63, 50, 4, .5],
  [68, 30, 2, .55], [72, 44, 3, .6], [75, 28, 2, .4],
  [78, 39, 2, .7], [83, 46, 4, 1],
] as const

export default function AuthLayout({ title, children }: { title: string; children: ReactNode }) {
  return (
    <main className="auth-page">
      <div className="light-stage" aria-hidden="true">
        <div className="ambient-glow" />
        <div className="light-beams">
          {beams.map(([x, height, width, opacity], index) => (
            <span key={x} className="light-beam" style={{
              left: `${x}%`, height: `${height}%`, width: `${width}px`,
              opacity, animationDelay: `${index * -.31}s`,
            }} />
          ))}
        </div>
        <div className="horizon" />
        <div className="floor-grid" />
        <div className="floor-ray floor-ray-left" />
        <div className="floor-ray floor-ray-right" />
      </div>
      <div className="brand-plate">
        <img
          src="/assets/flwmelogo.png"
          alt="FLWME"
          className="auth-brand-logo"
        />
      </div>
      <section className="login-card" aria-labelledby="auth-title">
        <h1 id="auth-title">{title}</h1>
        {children}
      </section>
    </main>
  )
}
