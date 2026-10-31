export type Credentials = { email: string; password: string }
export type User = { email: string }

type ErrorPayload = { error?: string }

export async function authRequest<T>(
  route: string,
  method: 'GET' | 'POST',
  body?: Credentials,
): Promise<T> {
  const response = await fetch(`/api/auth/${route}`, {
    method,
    credentials: 'same-origin',
    cache: 'no-store',
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!response.ok) {
    const data = (await response.json().catch(() => ({}))) as ErrorPayload
    throw new Error(data.error ?? `Request failed (${response.status})`)
  }
  return response.json() as Promise<T>
}
