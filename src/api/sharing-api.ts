import { run } from "./platform-api"
import type { PlatformState } from "@/types/platform"
export interface SharingInvitation { version:number; address:string; certificate:string; token:string; expiresAt:number }
export interface SharingGrant { id:string; environmentId:string; permission:"view"|"control"; address:string; expiresAt:number }
const native=()=>{throw new Error("Cross-computer sharing requires Yougori Desktop")}
export const sharingApi={
  list:()=>run<SharingGrant[]>("list_environment_shares",{},()=>[]),
  create:(environmentId:string,address:string,permission:"view"|"control")=>run<{share:SharingGrant;invitation:SharingInvitation}>("create_environment_share",{environmentId,address,permission},native),
  revoke:(shareId:string)=>run<void>("revoke_environment_share",{shareId},native),
  import:(invitation:SharingInvitation)=>run<PlatformState>("import_environment_share",{invitation},native),
}
