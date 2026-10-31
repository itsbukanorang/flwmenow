import {
  type FormEvent,
  useMemo,
  useState,
} from 'react'

import {
  AnimatePresence,
  motion,
} from 'framer-motion'

import {
  ArrowUpRight,
  Building2,
  Pencil,
  Plus,
  Search,
  SquareStack,
  Trash2,
  X,
} from 'lucide-react'

import DataTable, {
  type DataTableColumn,
} from './DataTable'

import {
  createCompany,
  deleteCompany,
  type Company,
  updateCompany,
} from '../lib/companies'

type Props = {
  companies: Company[]
  onCreated: (company: Company) => void
  onUpdated: (company: Company) => void
  onDeleted: (companyId: number) => void
  onOpenWorkflow: (company: Company) => void
}

export default function MyCompanies({
  companies,
  onCreated,
  onUpdated,
  onDeleted,
  onOpenWorkflow,
}: Props) {
  const [search, setSearch] = useState('')
  const [modalOpen, setModalOpen] = useState(false)
  const [editingCompany, setEditingCompany] =
    useState<Company | null>(null)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase()

    if (!needle) return companies

    return companies.filter((company) =>
      company.name.toLowerCase().includes(needle),
    )
  }, [companies, search])

  const openCreate = () => {
    setEditingCompany(null)
    setName('')
    setError('')
    setModalOpen(true)
  }

  const openEdit = (company: Company) => {
    setEditingCompany(company)
    setName(company.name)
    setError('')
    setModalOpen(true)
  }

  const handleDelete = async (company: Company) => {
    const ok = window.confirm(
      `Delete "${company.name}"? This cannot be undone.`,
    )

    if (!ok) return

    try {
      await deleteCompany(company.id)
      onDeleted(company.id)
    } catch (caught) {
      window.alert(
        caught instanceof Error
          ? caught.message
          : 'Unable to delete company',
      )
    }
  }

  const columns: DataTableColumn<Company>[] = [
    {
      key: 'index',
      header: 'No.',
      render: (company) => (
        <span className="company-row-number">
          {filtered.findIndex(
            (entry) => entry.id === company.id,
          ) + 1}
        </span>
      ),
    },
    {
      key: 'name',
      header: 'Company Name',
      render: (company) => (
        <div className="company-name-cell">
          <span>
            <Building2 size={16} />
          </span>

          <strong>{company.name}</strong>
        </div>
      ),
    },
    {
      key: 'status',
      header: 'Status',
      render: (company) => (
        <span
          className={`company-status-badge company-status-${company.status}`}
        >
          {(company.status ?? 'active') === 'active'
            ? 'Active'
            : (company.status ?? 'active') === 'paused'
              ? 'Paused'
              : company.status}
        </span>
      ),
    },
    {
      key: 'action',
      header: 'Action',
      render: (company) => (
        <div className="company-action-cell">
          <button
            type="button"
            className="company-action-edit"
            onClick={() => openEdit(company)}
          >
            <Pencil size={14} />
            Edit
          </button>

          <button
            type="button"
            className="company-action-stop"
            disabled
            title="Workflow stop control will be available from Workflow Studio."
          >
            <SquareStack size={14} />
            Stop All
          </button>

          <button
            type="button"
            className="company-action-delete"
            onClick={() => handleDelete(company)}
          >
            <Trash2 size={14} />
            Delete
          </button>

          <button
            type="button"
            className="company-open-workflow"
            title="Open Workflow Studio"
            onClick={() => onOpenWorkflow(company)}
          >
            <ArrowUpRight size={14} />
          </button>
        </div>
      ),
    },
  ]

  const submit = async (
    event: FormEvent<HTMLFormElement>,
  ) => {
    event.preventDefault()

    const cleanName = name.trim()

    if (cleanName.length < 2) {
      setError(
        'Company name must be at least 2 characters.',
      )
      return
    }

    setBusy(true)
    setError('')

    try {
      if (editingCompany) {
        const updated = await updateCompany(
          editingCompany.id,
          cleanName,
        )

        onUpdated(updated)
      } else {
        const created = await createCompany(cleanName)
        onCreated(created)
      }

      setName('')
      setEditingCompany(null)
      setModalOpen(false)
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : 'Unable to save company',
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <motion.section
      className="companies-page"
      initial={{ opacity: 0, y: 14 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{
        duration: 0.24,
        ease: 'easeOut',
      }}
    >
      <div className="companies-stats-row companies-stats-row-3">
        <article className="clone-glass-card company-summary-card">
          <div>
            <span className="clone-circle-icon">
              <Building2 size={17} />
            </span>

            <div>
              <small>Total Companies</small>
              <strong>{companies.length}</strong>
            </div>
          </div>
        </article>

        <article className="clone-glass-card company-summary-card">
          <div>
            <span className="clone-circle-icon">
              <SquareStack size={17} />
            </span>

            <div>
              <small>Running Task</small>
              <strong>0</strong>
            </div>
          </div>
        </article>

        <article className="clone-glass-card company-summary-card">
          <div>
            <span className="clone-circle-icon">
              <Pencil size={17} />
            </span>

            <div>
              <small>Need Action</small>
              <strong>0</strong>
            </div>
          </div>
        </article>
      </div>

      <div className="clone-glass-card company-action-bar">
        <div className="company-search">
          <Search size={16} />

          <input
            type="search"
            value={search}
            onChange={(event) =>
              setSearch(event.target.value)
            }
            placeholder="Search companies..."
          />
        </div>

        <button
          type="button"
          className="company-create-button"
          onClick={openCreate}
        >
          <Plus size={16} />
          Create Company
        </button>
      </div>

      <div className="companies-table-block">
        <DataTable
          columns={columns}
          data={filtered}
          emptyMessage={
            search
              ? 'No companies match your search.'
              : 'No companies yet. Create your first company.'
          }
        />
      </div>

      <AnimatePresence>
        {modalOpen && (
          <motion.div
            className="company-modal-backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onMouseDown={(event) => {
              if (event.target === event.currentTarget) {
                setModalOpen(false)
              }
            }}
          >
            <motion.div
              className="company-modal"
              initial={{
                opacity: 0,
                y: 18,
                scale: 0.96,
              }}
              animate={{
                opacity: 1,
                y: 0,
                scale: 1,
              }}
              exit={{
                opacity: 0,
                y: 10,
                scale: 0.97,
              }}
              transition={{
                type: 'spring',
                stiffness: 320,
                damping: 26,
              }}
            >
              <div className="company-modal-header">
                <div>
                  <small>
                    {editingCompany
                      ? 'UPDATE ORGANIZATION'
                      : 'NEW ORGANIZATION'}
                  </small>

                  <h2>
                    {editingCompany
                      ? 'Edit Company'
                      : 'Create Company'}
                  </h2>
                </div>

                <button
                  type="button"
                  onClick={() => setModalOpen(false)}
                  aria-label="Close"
                >
                  <X size={17} />
                </button>
              </div>

              <form onSubmit={submit}>
                <label htmlFor="company-name">
                  Company Name
                </label>

                <div className="company-input">
                  <Building2 size={16} />

                  <input
                    id="company-name"
                    type="text"
                    autoFocus
                    maxLength={120}
                    value={name}
                    onChange={(event) =>
                      setName(event.target.value)
                    }
                    placeholder="Enter company name"
                    required
                  />
                </div>

                {error && (
                  <p className="company-form-error">
                    {error}
                  </p>
                )}

                <div className="company-form-actions">
                  <button
                    type="button"
                    onClick={() => setModalOpen(false)}
                    disabled={busy}
                  >
                    Cancel
                  </button>

                  <button
                    type="submit"
                    className="primary"
                    disabled={busy}
                  >
                    {busy
                      ? editingCompany
                        ? 'Saving...'
                        : 'Creating...'
                      : editingCompany
                        ? 'Save Changes'
                        : 'Create Company'}
                  </button>
                </div>
              </form>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.section>
  )
}
