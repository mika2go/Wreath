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

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "Wreath"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "FileDescription" "Wreath low-overhead replay recorder installer"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Wreath contributors"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Wreath" MainSection
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"

  File /oname=wreath-win-ui.exe "${BINDIR}\wreath-win-ui.exe"
  File /oname=wreathd.exe "${BINDIR}\wreathd.exe"
  File /oname=wreathctl.exe "${BINDIR}\wreathctl.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

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
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wreath"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Wreath"
  DeleteRegKey /ifempty HKCU "Software\Wreath"

  Delete "$SMPROGRAMS\Wreath\Wreath.lnk"
  Delete "$SMPROGRAMS\Wreath\Uninstall Wreath.lnk"
  RMDir "$SMPROGRAMS\Wreath"

  Delete /REBOOTOK "$INSTDIR\wreath-win-ui.exe"
  Delete /REBOOTOK "$INSTDIR\wreathd.exe"
  Delete /REBOOTOK "$INSTDIR\wreathctl.exe"
  Delete /REBOOTOK "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
