; Mewsic — NSIS installer
; Build: makensis mewsic.nsi  (with mewsic.exe in the same directory)

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WordFunc.nsh"

Name "Mewsic"
OutFile "mewsic-setup.exe"
InstallDir "$PROGRAMFILES\Mewsic"
RequestExecutionLevel admin

!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "mewsic.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Add install dir to the system PATH
  ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path"
  ${If} $0 != ""
    WriteRegExpandStr HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path" "$0;$INSTDIR"
  ${Else}
    WriteRegExpandStr HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path" "$INSTDIR"
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000

  ; Start menu shortcut
  CreateDirectory "$SMPROGRAMS\Mewsic"
  CreateShortcut "$SMPROGRAMS\Mewsic\Uninstall Mewsic.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  ; Remove install dir from the system PATH
  ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path"
  ${WordReplace} "$0" ";$INSTDIR" "" "+" $0
  ${WordReplace} "$0" "$INSTDIR;" "" "+" $0
  WriteRegExpandStr HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path" "$0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000

  Delete "$INSTDIR\mewsic.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Mewsic\Uninstall Mewsic.lnk"
  RMDir "$SMPROGRAMS\Mewsic"
  DeleteRegKey HKCU "Software\Mewsic"
SectionEnd
