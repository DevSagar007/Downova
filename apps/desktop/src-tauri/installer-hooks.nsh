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
;
; ADR 0039: the program lives in %LOCALAPPDATA%\Programs\Downova, apart from the data in
; %LOCALAPPDATA%\Downova. An older installation in the data folder (recorded in the product key)
; is moved: its own service is stopped, its browser registration removed, and only its program
; files are deleted. The data (downova.sqlite3, run\, update\, native-messaging\) stays.

; The old folder when this run moved an installation out of the data folder; empty otherwise.
Var DownovaOldDir

!macro NSIS_HOOK_PREINSTALL
  StrCpy $DownovaOldDir ""
  ReadRegStr $R9 SHCTX "${MANUPRODUCTKEY}" ""
  ${If} $R9 == "$LOCALAPPDATA\${PRODUCTNAME}"
  ${AndIf} $INSTDIR != $R9
    StrCpy $DownovaOldDir $R9
    ${If} ${FileExists} "$R9\dm.exe"
      nsExec::ExecToLog '"$R9\dm.exe" shutdown --only-from "$R9"'
      Pop $0
      DetailPrint "Download service (old folder): dm shutdown --only-from exited $0"
    ${EndIf}
    ${If} ${FileExists} "$R9\dm-browser-host.exe"
      nsExec::ExecToLog '"$R9\dm-browser-host.exe" unregister --browser chrome --browser edge --browser firefox --host-path "$R9\dm-browser-host.exe"'
      Pop $0
      DetailPrint "Browser integration (old folder): dm-browser-host unregister exited $0"
    ${EndIf}
    !insertmacro CheckIfAppIsRunning "$R9\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    Delete "$R9\${MAINBINARYNAME}.exe"
    Delete "$R9\dm.exe"
    Delete "$R9\dm-service.exe"
    Delete "$R9\dm-browser-host.exe"
    Delete "$R9\THIRD-PARTY-NOTICES.txt"
    Delete "$R9\uninstall.exe"
    DetailPrint "Moved the program from $R9 to $INSTDIR; the data stays in $R9"
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\dm.exe"
    nsExec::ExecToLog '"$INSTDIR\dm.exe" shutdown --only-from "$INSTDIR"'
    Pop $0
    DetailPrint "Download service: dm shutdown --only-from exited $0"
  ${EndIf}
!macroend

!macro DownovaRetarget shortcut
  !insertmacro IsShortcutTarget "${shortcut}" "$DownovaOldDir\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "${shortcut}" "$INSTDIR\${MAINBINARYNAME}.exe"
    DetailPrint "Shortcut moved to the new folder: ${shortcut}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; ADR 0039: an update (/UPDATE) does not recreate shortcuts, so the existing ones that named
  ; the old folder are pointed at the moved program.
  ${If} $DownovaOldDir != ""
    !insertmacro DownovaRetarget "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    !insertmacro DownovaRetarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
    !insertmacro DownovaRetarget "$DESKTOP\${PRODUCTNAME}.lnk"
  ${EndIf}
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
