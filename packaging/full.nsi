; "Full" installer: the Ally interface and the desktop host in one file, with a
; tick box for each. Meant for testing both ends on one machine, or for one
; download that serves every machine.
;
; It does not install anything itself. It carries the two ordinary installers
; and runs the chosen ones quietly, so a machine set up this way is exactly
; like one set up with the separate downloads, and updates either way.
;
; Built by packaging/package.mjs, which passes:
;   VERSION     three numbers, e.g. 0.5.0
;   ALLY_SETUP  path of the Ally installer
;   HOST_SETUP  path of the desktop host installer
;   ICON        path of the CarlaOS icon
;   OUTFILE     where to write the installer

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
!include "WinMessages.nsh"

!ifndef VERSION | ALLY_SETUP | HOST_SETUP | ICON | OUTFILE
  !error "VERSION, ALLY_SETUP, HOST_SETUP, ICON and OUTFILE must be given. Use packaging/package.mjs."
!endif

!define PRODUCT "CarlaOS"
; Written by the two installers this one runs.
!define ALLY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\CarlaOS"
!define HOST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\CarlaOS Desktop Host"

Name "${PRODUCT} ${VERSION}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
SetCompressor lzma
ManifestDPIAware true

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} full installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "CarlaOS"

!define MUI_ICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC

!define MUI_WELCOMEPAGE_TITLE "${PRODUCT} ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "This installs either or both halves of CarlaOS on this machine:$\r$\n$\r$\n- the interface for the Ally (or a laptop standing in for it)$\r$\n- the desktop host, for a PC your games run on$\r$\n$\r$\nChoose on the next page. Both are ticked to start with, which is what you want for trying everything on one machine."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES

; Two tick boxes on the last page. The second borrows the "show readme" box.
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Set up this machine as a desktop host and show a code for the Ally"
!define MUI_FINISHPAGE_RUN_FUNCTION RunHostSetup
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Open CarlaOS (it fills the screen)"
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION RunAlly
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FinishShow
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

; Run one of the carried installers quietly and stop if it did not succeed.
!macro RunSetup FILE SOURCE WHAT
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=${FILE}" "${SOURCE}"
  DetailPrint "Installing ${WHAT}..."
  ExecWait '"$PLUGINSDIR\${FILE}" /S' $0
  Delete "$PLUGINSDIR\${FILE}"
  ${If} $0 != 0
    DetailPrint "${WHAT} was not installed (code $0)."
    SetErrorLevel 1
    MessageBox MB_ICONSTOP|MB_OK "${WHAT} could not be installed (code $0). Nothing else was changed by this step." /SD IDOK
    Abort
  ${EndIf}
  DetailPrint "${WHAT} is installed."
!macroend

Section "CarlaOS for the Ally" SecAlly
  !insertmacro RunSetup "ally-setup.exe" "${ALLY_SETUP}" "CarlaOS for the Ally"
SectionEnd

Section "CarlaOS Desktop Host" SecHost
  !insertmacro RunSetup "host-setup.exe" "${HOST_SETUP}" "CarlaOS Desktop Host"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAlly} "The pink console interface: library, games, streaming from your desktops. For the Ally or a laptop standing in for it."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecHost} "The background helper for a PC your games run on. It shows the Ally this PC's games and starts them for a stream."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; Each box only makes sense when its half was installed.
Function FinishShow
  ${IfNot} ${SectionIsSelected} ${SecHost}
    SendMessage $mui.FinishPage.Run ${BM_SETCHECK} ${BST_UNCHECKED} 0
    ShowWindow $mui.FinishPage.Run ${SW_HIDE}
  ${EndIf}
  ${IfNot} ${SectionIsSelected} ${SecAlly}
    SendMessage $mui.FinishPage.ShowReadme ${BM_SETCHECK} ${BST_UNCHECKED} 0
    ShowWindow $mui.FinishPage.ShowReadme ${SW_HIDE}
  ${EndIf}
FunctionEnd

Function RunHostSetup
  ReadRegStr $0 HKCU "${HOST_KEY}" "InstallLocation"
  ${If} $0 != ""
    Exec '"$0\carla-companion.exe"'
  ${EndIf}
FunctionEnd

Function RunAlly
  ; The Ally installer records its program, in quotes, as the icon to show.
  ReadRegStr $0 HKCU "${ALLY_KEY}" "DisplayIcon"
  ${If} $0 != ""
    Exec '$0'
  ${EndIf}
FunctionEnd
