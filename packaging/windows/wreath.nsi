Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!ifndef VERSION
  !error "VERSION must be supplied by the build script"
!endif
!ifndef BINDIR
  !error "BINDIR must be supplied by the build script"
!endif
!ifndef OUTFILE
  !error "OUTFILE must be supplied by the build script"
!endif

!include "MUI2.nsh"

Name "Wreath"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Wreath"
InstallDirRegKey HKCU "Software\Wreath" "InstallDir"
BrandingText "Wreath"
Icon "${__FILEDIR__}\wreath.ico"
UninstallIcon "${__FILEDIR__}\wreath.ico"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "Wreath"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "FileDescription" "Wreath low-overhead replay recorder installer"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Wreath contributors"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\wreath-win-ui.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Wreath"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

!macro StopWreathProcesses
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wreath-win-ui.exe"'
  Pop $1
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wreath-tray.exe"'
  Pop $1
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wreathd.exe"'
  Pop $1
  ; A tray or UI that was still completing its startup can outlive the first
  ; process-tree sweep briefly. Run a second synchronous sweep so silent
  ; upgrades and uninstalls never leave the binaries mapped in memory.
  Sleep 500
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wreath-win-ui.exe"'
  Pop $1
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wreath-tray.exe"'
  Pop $1
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wreathd.exe"'
  Pop $1
  Sleep 500
!macroend

Section "Wreath" MainSection
  SectionIn RO
  SetShellVarContext current

  ; v0.1.x used wreath-win-ui.exe as the tray. Stop both old and new process
  ; layouts before replacing files so upgrades cannot retain the legacy tray
  ; mutex or leave the old executable mapped in memory.
  !insertmacro StopWreathProcesses

  SetOutPath "$INSTDIR"

  File /oname=wreath-win-ui.exe "${BINDIR}\wreath-win-ui.exe"
  File /oname=wreath-tray.exe "${BINDIR}\wreath-tray.exe"
  File /oname=wreathd.exe "${BINDIR}\wreathd.exe"
  File /oname=wreathctl.exe "${BINDIR}\wreathctl.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Preserve an existing opt-in while migrating the old UI-based autostart.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wreath"
  StrCmp $0 "" autostart_migrated
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wreath" '"$INSTDIR\wreath-tray.exe"'
  autostart_migrated:

  CreateDirectory "$SMPROGRAMS\Wreath"
  CreateShortcut "$SMPROGRAMS\Wreath\Wreath.lnk" "$INSTDIR\wreath-win-ui.exe" "" "$INSTDIR\wreath-win-ui.exe" 0
  CreateShortcut "$SMPROGRAMS\Wreath\Uninstall Wreath.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\Wreath" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "DisplayName" "Wreath"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "DisplayIcon" "$INSTDIR\wreath-win-ui.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "Publisher" "Wreath"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current

  ; Ask the recorder to shut down cleanly, then force-stop every Wreath process
  ; as a fallback before deleting the installed binaries.
  nsExec::ExecToLog '"$INSTDIR\wreathctl.exe" shutdown'
  Pop $1
  !insertmacro StopWreathProcesses

  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wreath"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath"
  DeleteRegKey /ifempty HKCU "Software\Wreath"

  Delete "$SMPROGRAMS\Wreath\Wreath.lnk"
  Delete "$SMPROGRAMS\Wreath\Uninstall Wreath.lnk"
  RMDir "$SMPROGRAMS\Wreath"

  ; First try an immediate delete. A force-stopped process can disappear from
  ; the process table just before Windows releases its executable mapping, so
  ; wait briefly and retry before falling back to a reboot-time deletion.
  Delete "$INSTDIR\wreath-win-ui.exe"
  Delete "$INSTDIR\wreath-tray.exe"
  Delete "$INSTDIR\wreathd.exe"
  Delete "$INSTDIR\wreathctl.exe"
  Sleep 1000
  Delete /REBOOTOK "$INSTDIR\wreath-win-ui.exe"
  Delete /REBOOTOK "$INSTDIR\wreath-tray.exe"
  Delete /REBOOTOK "$INSTDIR\wreathd.exe"
  Delete /REBOOTOK "$INSTDIR\wreathctl.exe"
  Delete /REBOOTOK "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
