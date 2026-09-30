import { TriangleAlertIcon } from "lucide-react"

export function CloudDeploymentWarning() {
  return <aside aria-label="Cloud deployment risks" className="space-y-2 rounded-lg border border-amber-500/35 bg-amber-500/5 p-3 text-xs">
    <p className="flex items-center gap-2 font-medium"><TriangleAlertIcon className="size-4 shrink-0" aria-hidden="true" />Before you deploy</p>
    <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
      <li>Your provider charges for VMs, disks, snapshots and transfers. Charges can continue after a failure or after stopping a VM.</li>
      <li>Deployment or copying can fail or leave an environment that does not boot. Keep an independent backup and verify files, applications and access before relying on the result.</li>
      <li>Review the selected image, account and firewall rules. Copied disks can contain passwords, keys and other private data.</li>
      <li>If an operation fails, inspect its recorded resources before retrying. Closing Yougori or removing a connection does not delete cloud resources.</li>
    </ul>
  </aside>
}

export const cloudRiskAcknowledgement = "I understand the costs and failure risks, have backed up important data, and will verify the deployment."
