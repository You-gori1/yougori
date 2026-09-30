import { useEffect, useRef, useState } from "react"
import { vaultApi } from "@/api/vault-api"

export function useVaultStartup(platformReady: boolean) {
  const [ready, setReady] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)
  const startup = useRef<ReturnType<typeof vaultApi.bootstrap> | null>(null)
  useEffect(() => {
    if (!platformReady) return
    let active = true
    startup.current ??= vaultApi.bootstrap()
    void startup.current.then(result => {
      if (result.created && !result.ready && !result.updateRequired) throw new Error("The vault container is not ready yet")
      if (active) setReady(true)
    }).catch(reason => {
      if (active) setError(`Could not start Personal Vault: ${reason instanceof Error ? reason.message : String(reason)}`)
    })
    return () => { active = false }
  }, [platformReady, attempt])
  return {
    ready,
    error,
    retry: () => { startup.current = null; setError(null); setReady(false); setAttempt(value => value + 1) },
    continueWithoutVault: () => { if (error) setReady(true) },
  }
}
