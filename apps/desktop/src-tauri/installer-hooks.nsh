; B2.5 (ADR 0014): per-user native-messaging registration for the NSIS installer.
; The installer runs per user (installMode currentUser, no elevation), so HKCU is the
; installing user's. The host checks ownership itself: it never replaces another program's
; entry, and on uninstall it removes only entries naming this installation's host.
; Failures do not fail the install: the app registers again on its first launch and
; `dm-browser-host repair` can be run at any time.
;
; B3.6 (ADR 0031): the download service outlives the window by design, so before files are
; replaced (upgrade) or removed (uninstall) the service is stopped with its state kept, but only
; if it runs from THIS installation folder (`dm shutdown --only-from`); a service of another
; installation, or anything else named dm-service.exe, is left alone. Downloads continue from
; their checkpoints at the next start. The user's data folder is never deleted here.

!macro NSIS_HOOK_PREINSTALL
  ${If} ${FileExists} "$INSTDIR\dm.exe"
    nsExec::ExecToLog '"$INSTDIR\dm.exe" shutdown --only-from "$INSTDIR"'
    Pop $0
    DetailPrint "Download service: dm shutdown --only-from exited $0"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\dm-browser-host.exe" register --browser chrome --browser edge --browser firefox'
  Pop $0
  DetailPrint "Browser integration: dm-browser-host register exited $0"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\dm.exe" shutdown --only-from "$INSTDIR"'
  Pop $0
  DetailPrint "Download service: dm shutdown --only-from exited $0"
  nsExec::ExecToLog '"$INSTDIR\dm-browser-host.exe" unregister --browser chrome --browser edge --browser firefox --host-path "$INSTDIR\dm-browser-host.exe"'
  Pop $0
  DetailPrint "Browser integration: dm-browser-host unregister exited $0"
!macroend
