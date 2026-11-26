; battwarn installer - per-user, no admin required.
; Build with:  makensis installer/battwarn.nsi   (see build.sh)

Unicode true
!include "MUI2.nsh"

!define APPNAME "battwarn"
!define VERSION "1.0.0"

Name "${APPNAME}"
Caption "${APPNAME} setup"
OutFile "../dist/battwarn-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\battwarn"
InstallDirRegKey HKCU "Software\${APPNAME}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 32
ShowInstDetails show

VIProductVersion "1.0.0.0"
VIAddVersionKey "ProductName" "battwarn"
VIAddVersionKey "FileDescription" "battwarn installer"
VIAddVersionKey "FileVersion" "1.0.0.0"
VIAddVersionKey "ProductVersion" "1.0.0"
VIAddVersionKey "LegalCopyright" "MIT"

!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "battwarn"
!define MUI_WELCOMEPAGE_TEXT "Watches your battery and pops up a warning before Windows can shut down without one.$\r$\n$\r$\nInstalls for the current user only. No administrator rights needed."
!define MUI_FINISHPAGE_RUN "$INSTDIR\battwarn.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Show battery status now"
!define MUI_FINISHPAGE_TEXT "battwarn is installed and already watching.$\r$\n$\r$\nIt starts by itself every time you log in.$\r$\n$\r$\nIn a terminal:$\r$\n    battwarn          battery status$\r$\n    battwarn test     show the warning popup$\r$\n    battwarn uninstall$\r$\n$\r$\nChange the threshold with: battwarn install -t 25"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Section "battwarn"
  SetOutPath "$INSTDIR"
  File "../dist/battwarn.exe"
  WriteRegStr HKCU "Software\${APPNAME}" "InstallDir" "$INSTDIR"

  ; All the real work (PATH, autostart, watcher, Add/Remove entry) lives in
  ; the binary itself, so a bare battwarn.exe can do the same job.
  ExecWait '"$INSTDIR\battwarn.exe" install --quiet'

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  ; Strip autostart, PATH, the running watcher and the Add/Remove entry,
  ; but leave the files for us to delete below.
  ExecWait '"$INSTDIR\battwarn.exe" uninstall --keep-files --quiet'
  Delete "$INSTDIR\battwarn.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\${APPNAME}"
SectionEnd
