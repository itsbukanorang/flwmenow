import type { ReactNode } from 'react'

export type DataTableColumn<T> = {
  key: string
  header: string
  render?: (item: T) => ReactNode
}

type Props<T> = {
  columns: DataTableColumn<T>[]
  data: T[]
  emptyMessage: string
}

export default function DataTable<
  T extends { id: number | string },
>({
  columns,
  data,
  emptyMessage,
}: Props<T>) {
  return (
    <>
      <div className="company-table-desktop">
        <div className="company-table-shell">
          <table>
            <thead>
              <tr>
                {columns.map((column) => (
                  <th key={column.key}>
                    {column.header}
                  </th>
                ))}
              </tr>
            </thead>

            <tbody>
              {data.length === 0 ? (
                <tr>
                  <td
                    colSpan={columns.length}
                    className="company-table-empty"
                  >
                    {emptyMessage}
                  </td>
                </tr>
              ) : (
                data.map((item, index) => (
                  <tr
                    key={item.id}
                    className="company-table-row"
                    style={{
                      animationDelay: `${index * 45}ms`,
                    }}
                  >
                    {columns.map((column) => (
                      <td key={column.key}>
                        {column.render
                          ? column.render(item)
                          : (
                              item as Record<
                                string,
                                ReactNode
                              >
                            )[column.key]}
                      </td>
                    ))}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      <div className="company-table-mobile">
        {data.length === 0 ? (
          <div className="company-mobile-empty">
            {emptyMessage}
          </div>
        ) : (
          data.map((item, index) => (
            <article
              key={item.id}
              className="company-mobile-card"
              style={{
                animationDelay: `${index * 45}ms`,
              }}
            >
              {columns.map((column) => (
                <div key={column.key}>
                  <small>{column.header}</small>

                  <div>
                    {column.render
                      ? column.render(item)
                      : (
                          item as Record<
                            string,
                            ReactNode
                          >
                        )[column.key]}
                  </div>
                </div>
              ))}
            </article>
          ))
        )}
      </div>
    </>
  )
}
