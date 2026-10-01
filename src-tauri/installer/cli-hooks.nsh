!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\third-party-sources\scripts\install-cli-path.ps1" -InstallDirectory "$INSTDIR" -Action Install'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Yougori CLI PATH setup failed. Add $INSTDIR\cli to your user PATH manually."
  ${EndIf}
  nsExec::ExecToLog '"$INSTDIR\cli\yougori.exe" skills install'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Yougori skill setup could not finish. Existing custom skills were preserved; run yougori skills install to retry."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\third-party-sources\scripts\install-cli-path.ps1" -InstallDirectory "$INSTDIR" -Action Uninstall'
  Pop $0
!macroend
