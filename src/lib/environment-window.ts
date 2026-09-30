const ENVIRONMENT_QUERY_PARAMETER = "environment"

export function environmentIdFromSearch(search: string) {
  const value = new URLSearchParams(search).get(ENVIRONMENT_QUERY_PARAMETER)?.trim()
  return value && value.length <= 80 && /^env-[a-zA-Z0-9_-]+$/.test(value) ? value : null
}
