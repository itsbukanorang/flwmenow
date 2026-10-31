import {
  useEffect,
  useMemo,
  useState,
} from 'react'

import {
  AnimatePresence,
  motion,
} from 'framer-motion'

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

import MyCompanies from '../components/MyCompanies'

import WorkflowStudio from '../components/WorkflowStudio'

import {
  authRequest,
  type User,
} from '../lib/auth'

import {
  getCompanies,
  type Company,
} from '../lib/companies'

import '../styles/dashboard-v1.css'

type Section =
  | 'dashboard'
  | 'companies'
  | 'workflow'

function userNameFromEmail(email: string) {
  const local = email.split('@')[0] ?? ''
  const first =
    local.split(/[._-]/).filter(Boolean)[0] ??
    'User'

  return (
    first.charAt(0).toUpperCase() +
    first.slice(1)
  )
}

function MetricCard({
  icon,
  title,
  value = '—',
  primaryLabel,
  secondaryLabel,
  onOpen,
}: {
  icon: React.ReactNode
  title: string
  value?: React.ReactNode
  primaryLabel: string
  secondaryLabel: string
  onOpen?: () => void
}) {
  return (
    <article className="clone-stat-card">
      <div className="clone-stat-top">
        <div className="clone-stat-title">
          <span className="clone-circle-icon">
            {icon}
          </span>

          <span>{title}</span>
        </div>

        <button
          className="clone-arrow-button"
          type="button"
          aria-label={`Open ${title}`}
          onClick={onOpen}
        >
          <ArrowUpRight size={15} />
        </button>
      </div>

      <strong className="clone-stat-value">
        {value}
      </strong>

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
  const [section, setSection] =
    useState<Section>('dashboard')

  const [user, setUser] =
    useState<User | null>(null)

  const [companies, setCompanies] =
    useState<Company[]>([])

  const [selectedCompany, setSelectedCompany] =
    useState<Company | null>(null)

  const [profileOpen, setProfileOpen] =
    useState(false)

  useEffect(() => {
    let active = true

    authRequest<User>('me', 'GET')
      .then(async (data) => {
        if (!active) return

        setUser(data)

        try {
          const items = await getCompanies()

          if (active) {
            setCompanies(items)
          }
        } catch {
          // ignore during rollout
        }
      })
      .catch(() => window.location.replace('/'))

    return () => {
      active = false
    }
  }, [])

  const displayName = useMemo(
    () =>
      userNameFromEmail(
        user?.email ?? '',
      ),
    [user],
  )

  const logout = async () => {
    try {
      await authRequest<{
        ok: boolean
      }>('logout', 'POST')
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
            <img
              src="/assets/flwme-logo.png"
              alt="FLWME"
              className="clone-brand-logo"
            />
          </div>

          <div className="clone-actions">
            <button
              type="button"
              className="clone-circle-button"
              aria-label="Settings"
            >
              <Settings size={17} />
            </button>

            <button
              type="button"
              className="clone-circle-button"
              aria-label="AI"
            >
              <Sparkles size={17} />
            </button>

            <button
              type="button"
              className="clone-circle-button"
              aria-label="Apps"
            >
              <Grid3X3 size={17} />
            </button>

            <button
              type="button"
              className="clone-circle-button"
              aria-label="Help"
            >
              <CircleHelp size={17} />
            </button>

            <button
              type="button"
              className="clone-circle-button"
              aria-label="Notifications"
            >
              <Bell size={17} />
            </button>

            <div className="clone-profile">
              <button
                type="button"
                className="clone-profile-trigger"
                onClick={() =>
                  setProfileOpen(
                    (value) => !value,
                  )
                }
              >
                <span className="clone-avatar">
                  {displayName.charAt(0)}
                </span>

                <ChevronDown size={13} />
              </button>

              {profileOpen && (
                <div className="clone-profile-menu">
                  <div className="clone-profile-copy">
                    <strong>
                      {displayName}
                    </strong>

                    <span>{user.email}</span>
                  </div>

                  <div className="clone-menu-separator" />

                  <button
                    type="button"
                    onClick={logout}
                  >
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
              {section === 'dashboard' &&
                'Dashboard Overview'}
              {section === 'companies' &&
                'My Companies'}
              {section === 'workflow' &&
                'Workflow Studio'}
            </strong>
          </div>

          <nav>
            <button
              type="button"
              className={
                section === 'dashboard'
                  ? 'active'
                  : ''
              }
              onClick={() =>
                setSection('dashboard')
              }
            >
              Dashboard
            </button>

            <button
              type="button"
              className={
                section === 'companies'
                  ? 'active'
                  : ''
              }
              onClick={() =>
                setSection('companies')
              }
            >
              My Companies
            </button>

          </nav>
        </div>
      </header>

      <main className="clone-main">
        <AnimatePresence mode="wait">
          <motion.div
            key={section}
            initial={{
              opacity: 0,
              y: 12,
            }}
            animate={{
              opacity: 1,
              y: 0,
            }}
            exit={{
              opacity: 0,
              y: -8,
            }}
            transition={{
              duration: 0.20,
              ease: 'easeOut',
            }}
          >
            {section === 'dashboard' && (
              <>
                <div className="clone-welcome">
                  Welcome Back,{' '}
                  <strong>
                    {displayName}
                  </strong>
                </div>

                <section className="clone-stats-grid">
                  <MetricCard
                    icon={
                      <Building2 size={18} />
                    }
                    title="My Companies"
                    value={companies.length}
                    primaryLabel="Active Companies"
                    secondaryLabel="New Companies"
                    onOpen={() =>
                      setSection('companies')
                    }
                  />

                  <MetricCard
                    icon={
                      <Workflow size={18} />
                    }
                    title="Workflows"
                    primaryLabel="Active Workflows"
                    secondaryLabel="Completed"
                    onOpen={() =>
                      setSection('companies')
                    }
                  />

                  <MetricCard
                    icon={<Bot size={18} />}
                    title="AI Workforce"
                    primaryLabel="Active Workers"
                    secondaryLabel="Executives"
                  />

                  <MetricCard
                    icon={
                      <Activity size={18} />
                    }
                    title="Activities"
                    primaryLabel="Running Tasks"
                    secondaryLabel="Completed"
                  />
                </section>

                <section className="clone-dashboard-grid">
                  <article className="clone-glass-card clone-large-panel">
                    <div className="clone-panel-header">
                      <h2>
                        Workspace Activity
                      </h2>

                      <div className="clone-panel-actions">
                        <button type="button">
                          7 Days
                        </button>
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
                        <strong>
                          No workspace activity yet
                        </strong>
                        <p>
                          Real company,
                          workflow and task
                          activity will appear
                          here.
                        </p>
                      </div>
                    </div>
                  </article>

                  <article className="clone-glass-card clone-large-panel">
                    <div className="clone-panel-header">
                      <h2>
                        Recent Activity
                      </h2>

                      <div className="clone-panel-actions">
                        <button type="button">
                          Latest
                        </button>
                      </div>
                    </div>

                    <div className="clone-list-empty">
                      <Activity size={22} />
                      <strong>
                        No recent activity
                      </strong>
                      <p>
                        Verified work and
                        execution history will
                        appear here.
                      </p>
                    </div>
                  </article>

                  <article className="clone-glass-card clone-bottom-panel">
                    <div className="clone-panel-header">
                      <div className="clone-tabs">
                        <button
                          type="button"
                          className="active"
                        >
                          My Companies
                        </button>

                        <button type="button">
                          Recent
                        </button>
                      </div>

                      <button
                        className="clone-arrow-button"
                        type="button"
                        onClick={() =>
                          setSection(
                            'companies',
                          )
                        }
                      >
                        <ArrowUpRight
                          size={15}
                        />
                      </button>
                    </div>

                    {companies.length === 0 ? (
                      <div className="clone-list-empty compact">
                        <Building2
                          size={21}
                        />
                        <strong>
                          No companies yet
                        </strong>
                        <p>
                          Create your first
                          company to start
                          building your
                          organization.
                        </p>
                      </div>
                    ) : (
                      <div className="dashboard-company-mini-list">
                        {companies
                          .slice(0, 4)
                          .map((company) => (
                            <div key={company.id}>
                              <span>
                                <Building2
                                  size={15}
                                />
                              </span>

                              <strong>
                                {company.name}
                              </strong>
                            </div>
                          ))}
                      </div>
                    )}
                  </article>

                  <article className="clone-glass-card clone-bottom-panel">
                    <div className="clone-panel-header">
                      <h2>
                        Workflow Overview
                      </h2>

                      <button
                        className="clone-arrow-button"
                        type="button"
                        onClick={() =>
                          setSection(
                            'workflow',
                          )
                        }
                      >
                        <ArrowUpRight
                          size={15}
                        />
                      </button>
                    </div>

                    <div className="clone-overview-empty">
                      <div className="clone-donut">
                        <span>—</span>
                      </div>

                      <div>
                        <strong>
                          No workflow data yet
                        </strong>

                        <p>
                          Workflow performance
                          will appear once your
                          studio begins
                          executing work.
                        </p>
                      </div>
                    </div>
                  </article>
                </section>
              </>
            )}

            {section === 'companies' && (
              <MyCompanies
                companies={companies}
                onCreated={(company) =>
                  setCompanies(
                    (current) => [
                      company,
                      ...current,
                    ],
                  )
                }
                onUpdated={(company) =>
                  setCompanies((current) =>
                    current.map((entry) =>
                      entry.id === company.id
                        ? company
                        : entry,
                    ),
                  )
                }
                onDeleted={(companyId) =>
                  setCompanies((current) =>
                    current.filter(
                      (entry) =>
                        entry.id !== companyId,
                    ),
                  )
                }
                onOpenWorkflow={(company) => {
                  setSelectedCompany(company)
                  setSection('workflow')
                }}
              />
            )}

            {section === 'workflow' && (
              <WorkflowStudio
                company={selectedCompany}
                onBack={() =>
                  setSection('companies')
                }
              />
            )}
          </motion.div>
        </AnimatePresence>
      </main>
    </div>
  )
}
