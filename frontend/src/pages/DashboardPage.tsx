import { useEffect, useMemo, useState } from 'react'
import {
  Activity,
  ArrowUpRight,
  Bell,
  Bot,
  Building2,
  ChevronDown,
  CircleHelp,
  Grid3X3,
  LayoutDashboard,
  LogOut,
  Settings,
  Sparkles,
  Workflow,
} from 'lucide-react'
import { authRequest, type User } from '../lib/auth'
import '../styles/dashboard-v1.css'

type Section = 'dashboard' | 'companies' | 'workflow'

function userNameFromEmail(email: string) {
  const local = email.split('@')[0] ?? ''
  const first = local.split(/[._-]/).filter(Boolean)[0] ?? 'User'
  return first.charAt(0).toUpperCase() + first.slice(1)
}

function MetricCard({
  icon,
  title,
  primaryLabel,
  secondaryLabel,
}: {
  icon: React.ReactNode
  title: string
  primaryLabel: string
  secondaryLabel: string
}) {
  return (
    <article className="clone-stat-card">
      <div className="clone-stat-top">
        <div className="clone-stat-title">
          <span className="clone-circle-icon">{icon}</span>
          <span>{title}</span>
        </div>

        <button className="clone-arrow-button" type="button" aria-label={`Open ${title}`}>
          <ArrowUpRight size={15} />
        </button>
      </div>

      <strong className="clone-stat-value">—</strong>

      <div className="clone-stat-divider" />

      <div className="clone-stat-bottom">
        <div>
          <small>{primaryLabel}</small>
          <strong>—</strong>
        </div>

        <div>
          <small>{secondaryLabel}</small>
          <strong>—</strong>
        </div>
      </div>
    </article>
  )
}

export default function DashboardPage() {
  const [section, setSection] = useState<Section>('dashboard')
  const [user, setUser] = useState<User | null>(null)
  const [profileOpen, setProfileOpen] = useState(false)

  useEffect(() => {
    let active = true

    authRequest<User>('me', 'GET')
      .then((data) => {
        if (active) setUser(data)
      })
      .catch(() => window.location.replace('/'))

    return () => {
      active = false
    }
  }, [])

  const displayName = useMemo(
    () => userNameFromEmail(user?.email ?? ''),
    [user],
  )

  const logout = async () => {
    try {
      await authRequest<{ ok: boolean }>('logout', 'POST')
    } finally {
      window.location.replace('/')
    }
  }

  if (!user) {
    return (
      <div className="clone-loading">
        <span />
      </div>
    )
  }

  return (
    <div className="clone-dashboard">
      <header className="clone-header">
        <div className="clone-topbar">
          <div className="clone-brand">
            <div className="clone-brand-mark">F</div>
            <span>FLWME</span>
          </div>

          <div className="clone-actions">
            <button type="button" className="clone-circle-button" aria-label="Settings">
              <Settings size={17} />
            </button>

            <button type="button" className="clone-circle-button" aria-label="AI">
              <Sparkles size={17} />
            </button>

            <button type="button" className="clone-circle-button" aria-label="Apps">
              <Grid3X3 size={17} />
            </button>

            <button type="button" className="clone-circle-button" aria-label="Help">
              <CircleHelp size={17} />
            </button>

            <button type="button" className="clone-circle-button" aria-label="Notifications">
              <Bell size={17} />
            </button>

            <div className="clone-profile">
              <button
                type="button"
                className="clone-profile-trigger"
                onClick={() => setProfileOpen((value) => !value)}
              >
                <span className="clone-avatar">{displayName.charAt(0)}</span>
                <ChevronDown size={13} />
              </button>

              {profileOpen && (
                <div className="clone-profile-menu">
                  <div className="clone-profile-copy">
                    <strong>{displayName}</strong>
                    <span>{user.email}</span>
                  </div>

                  <div className="clone-menu-separator" />

                  <button type="button" onClick={logout}>
                    <LogOut size={15} />
                    Log out
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>

        <div className="clone-navigation">
          <div className="clone-navigation-heading">
            <LayoutDashboard size={18} />
            <strong>
              {section === 'dashboard' && 'Dashboard Overview'}
              {section === 'companies' && 'My Companies'}
              {section === 'workflow' && 'Workflow Studio'}
            </strong>
          </div>

          <nav>
            <button
              type="button"
              className={section === 'dashboard' ? 'active' : ''}
              onClick={() => setSection('dashboard')}
            >
              Dashboard
            </button>

            <button
              type="button"
              className={section === 'companies' ? 'active' : ''}
              onClick={() => setSection('companies')}
            >
              My Companies
            </button>

            <button
              type="button"
              className={section === 'workflow' ? 'active' : ''}
              onClick={() => setSection('workflow')}
            >
              Workflow Studio
            </button>
          </nav>
        </div>
      </header>

      <main className="clone-main">
        {section === 'dashboard' && (
          <>
            <div className="clone-welcome">
              Welcome Back, <strong>{displayName}</strong>
            </div>

            <section className="clone-stats-grid">
              <MetricCard
                icon={<Building2 size={18} />}
                title="My Companies"
                primaryLabel="Active Companies"
                secondaryLabel="New Companies"
              />

              <MetricCard
                icon={<Workflow size={18} />}
                title="Workflows"
                primaryLabel="Active Workflows"
                secondaryLabel="Completed"
              />

              <MetricCard
                icon={<Bot size={18} />}
                title="AI Workforce"
                primaryLabel="Active Workers"
                secondaryLabel="Executives"
              />

              <MetricCard
                icon={<Activity size={18} />}
                title="Activities"
                primaryLabel="Running Tasks"
                secondaryLabel="Completed"
              />
            </section>

            <section className="clone-dashboard-grid">
              <article className="clone-glass-card clone-large-panel">
                <div className="clone-panel-header">
                  <h2>Workspace Activity</h2>

                  <div className="clone-panel-actions">
                    <button type="button">7 Days</button>
                  </div>
                </div>

                <div className="clone-chart-area">
                  <div className="clone-chart-grid">
                    <span />
                    <span />
                    <span />
                    <span />
                  </div>

                  <div className="clone-empty-copy">
                    <Activity size={22} />
                    <strong>No workspace activity yet</strong>
                    <p>
                      Real company, workflow and task activity will appear here.
                    </p>
                  </div>
                </div>
              </article>

              <article className="clone-glass-card clone-large-panel">
                <div className="clone-panel-header">
                  <h2>Recent Activity</h2>

                  <div className="clone-panel-actions">
                    <button type="button">Latest</button>
                  </div>
                </div>

                <div className="clone-list-empty">
                  <Activity size={22} />
                  <strong>No recent activity</strong>
                  <p>Verified work and execution history will appear here.</p>
                </div>
              </article>

              <article className="clone-glass-card clone-bottom-panel">
                <div className="clone-panel-header">
                  <div className="clone-tabs">
                    <button type="button" className="active">
                      My Companies
                    </button>

                    <button type="button">
                      Recent
                    </button>
                  </div>

                  <button className="clone-arrow-button" type="button">
                    <ArrowUpRight size={15} />
                  </button>
                </div>

                <div className="clone-list-empty compact">
                  <Building2 size={21} />
                  <strong>No companies yet</strong>
                  <p>Create your first company to start building your organization.</p>
                </div>
              </article>

              <article className="clone-glass-card clone-bottom-panel">
                <div className="clone-panel-header">
                  <h2>Workflow Overview</h2>

                  <button className="clone-arrow-button" type="button">
                    <ArrowUpRight size={15} />
                  </button>
                </div>

                <div className="clone-overview-empty">
                  <div className="clone-donut">
                    <span>—</span>
                  </div>

                  <div>
                    <strong>No workflow data yet</strong>
                    <p>
                      Workflow performance will appear once your studio begins executing work.
                    </p>
                  </div>
                </div>
              </article>
            </section>
          </>
        )}

        {section === 'companies' && (
          <section className="clone-glass-card clone-section-placeholder">
            <Building2 size={28} />
            <h1>My Companies</h1>
            <p>Company management is the next feature we build.</p>
          </section>
        )}

        {section === 'workflow' && (
          <section className="clone-glass-card clone-section-placeholder">
            <Workflow size={28} />
            <h1>Workflow Studio</h1>
            <p>Your cognitive workflow studio will live here.</p>
          </section>
        )}
      </main>
    </div>
  )
}
