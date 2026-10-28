; B2.5 (ADR 0014): per-user native-messaging registration for the NSIS installer.
; The installer runs per user (installMode currentUser, no elevation), so HKCU is the
; installing user's. The host checks ownership itself: it never replaces another program's
; entry, and on uninstall it removes only entries naming this installation's host.
; Failures do not fail the install: the app registers again on its first launch and
; `dm-browser-host repair` can be run at any time.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\dm-browser-host.exe" register --browser chrome --browser edge'
  Pop $0
  DetailPrint "Browser integration: dm-browser-host register exited $0"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\dm-browser-host.exe" unregister --browser chrome --browser edge --host-path "$INSTDIR\dm-browser-host.exe"'
  Pop $0
  DetailPrint "Browser integration: dm-browser-host unregister exited $0"
!macroend
