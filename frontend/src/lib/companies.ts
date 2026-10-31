export type Company = {
  id: number
  name: string
  status: string
  created_at: string
}

type CompanyList = {
  companies: Company[]
}

type ErrorPayload = {
  error?: string
}

async function request<T>(
  path: string,
  method: 'GET' | 'POST' | 'PUT' | 'DELETE',
  body?: unknown,
): Promise<T> {
  const response = await fetch(path, {
    method,
    credentials: 'same-origin',
    cache: 'no-store',
    headers: body
      ? { 'Content-Type': 'application/json' }
      : undefined,
    body: body ? JSON.stringify(body) : undefined,
  })

  if (!response.ok) {
    const payload = (await response
      .json()
      .catch(() => ({}))) as ErrorPayload

    throw new Error(
      payload.error ?? `Request failed (${response.status})`,
    )
  }

  if (response.status === 204) {
    return undefined as T
  }

  return response.json() as Promise<T>
}

export async function getCompanies() {
  const result = await request<CompanyList>(
    '/api/companies',
    'GET',
  )

  return result.companies
}

export async function createCompany(name: string) {
  return request<Company>(
    '/api/companies',
    'POST',
    { name },
  )
}

export async function updateCompany(
  companyId: number,
  name: string,
) {
  return request<Company>(
    `/api/companies/${companyId}`,
    'PUT',
    { name },
  )
}

export async function deleteCompany(
  companyId: number,
) {
  return request<{ ok: boolean }>(
    `/api/companies/${companyId}`,
    'DELETE',
  )
}
