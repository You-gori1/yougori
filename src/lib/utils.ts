// Derived from Coss UI (MIT). Upstream attribution and terms: ../components/ui/LICENSE.txt
import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
