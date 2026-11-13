; Installer for the CarlaOS desktop host: the companion that runs on each
; desktop PC that streams games to the Ally.
;
; Built by packaging/package.mjs, which passes:
;   VERSION    three numbers, e.g. 0.5.0
;   COMPANION  path of the built carla-companion.exe
;   ICON       path of the CarlaOS icon
;   OUTFILE    where to write the installer
;
; It installs for the signed-in account only, so Windows never asks for an
; administrator. The companion's own settings and links live elsewhere (the
; account's application data) and are left alone by installing, updating,
; going back to an older version and uninstalling.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

!ifndef VERSION | COMPANION | ICON | OUTFILE
  !error "VERSION, COMPANION, ICON and OUTFILE must be given. Use packaging/package.mjs."
!endif

!define PRODUCT "CarlaOS Desktop Host"
!define EXE "carla-companion.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
; Where the companion itself registers to start with Windows (crates/companion).
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define RUN_NAME "CarlaOS Companion"

Name "${PRODUCT} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ManifestDPIAware true

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "CarlaOS"

Var Updating

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TITLE "${PRODUCT} ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "This is for a desktop PC that your games run on. It lets the Ally see this PC's games and start them with one press.$\r$\n$\r$\nFor the Ally itself (or a laptop standing in for it), use the $\"CarlaOS for the Ally$\" download instead.$\r$\n$\r$\nBefore the first stream this PC also needs Sunshine. The set-up at the end tells you what it wants."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Set up this desktop and show a code for linking the Ally"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FinishShow
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; After an update there is nothing to set up, so the box starts unticked.
Function FinishShow
  ${If} $Updating == "1"
    SendMessage $mui.FinishPage.Run ${BM_SETCHECK} ${BST_UNCHECKED} 0
  ${EndIf}
FunctionEnd

Section "Install"
  ; Set up before = the companion registered itself to start with Windows.
  StrCpy $Updating "0"
  ReadRegStr $0 HKCU "${RUN_KEY}" "${RUN_NAME}"
  ${If} $0 != ""
    StrCpy $Updating "1"
  ${EndIf}

  ; A running companion keeps its file locked. Its settings are on disk, so
  ; stopping it loses nothing; a game that is being streamed keeps running.
  nsExec::Exec 'taskkill /F /IM ${EXE}'
  Pop $0
  Sleep 500

  SetOutPath "$INSTDIR"
  File "/oname=${EXE}" "${COMPANION}"
  File "/oname=carlaos.ico" "${ICON}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\carlaos.ico"
  CreateShortcut "$SMPROGRAMS\Update ${PRODUCT}.lnk" "$INSTDIR\${EXE}" "update" "$INSTDIR\carlaos.ico"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "CarlaOS"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\carlaos.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1

  ; Updating: start from this copy with Windows, and carry on in the
  ; background now so the Ally does not lose the desktop until the next sign-in.
  ${If} $Updating == "1"
    WriteRegStr HKCU "${RUN_KEY}" "${RUN_NAME}" 'conhost.exe --headless "$INSTDIR\${EXE}" serve'
    Exec 'conhost.exe --headless "$INSTDIR\${EXE}" serve'
  ${EndIf}
SectionEnd

Section "Uninstall"
  ; "remove" is the companion's own way of no longer starting with Windows.
  nsExec::Exec '"$INSTDIR\${EXE}" remove'
  Pop $0
  DeleteRegValue HKCU "${RUN_KEY}" "${RUN_NAME}"
  nsExec::Exec 'taskkill /F /IM ${EXE}'
  Pop $0
  Sleep 500

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\carlaos.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$SMPROGRAMS\Update ${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
