Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!ifndef APP_VERSION
!define APP_VERSION "0.1.0"
!endif
!ifndef PAYLOAD
!error "Specify PAYLOAD"
!endif
!ifndef OUTPUT
!error "Specify OUTPUT"
!endif
Name "Школьный день"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\SchoolDay"
InstallDirRegKey HKCU "Software\SchoolDay" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 32
BrandingText "Школьный день"
!define MUI_ICON "school-day.ico"
!define MUI_UNICON "school-day.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\bin\school-day.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Открыть приложение"
!define MUI_FINISHPAGE_LINK "GitHub"
!define MUI_FINISHPAGE_LINK_LOCATION "https://github.com/sqeezy-53/school-day"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Russian"
VIProductVersion "0.1.0.0"
VIAddVersionKey /LANG=1049 "ProductName" "Школьный день"
VIAddVersionKey /LANG=1049 "FileDescription" "Установка приложения Школьный день"
VIAddVersionKey /LANG=1049 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=1049 "LegalCopyright" "sqeezy-53"

!macro CHECK_RUNNING
check_running:
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\io.github.sqeezy_53.school-day") p .r0'
  IntCmp $0 0 not_running
  System::Call 'kernel32::CloseHandle(p r0)'
  IfSilent stop_install
  MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Закройте приложение «Школьный день» и нажмите «Повторить»." IDRETRY check_running
stop_install:
  SetErrorLevel 2
  Abort
not_running:
!macroend

Function .onInit
  !insertmacro CHECK_RUNNING
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP "Нужна Windows 10 или новее."
    Abort
  ${EndIf}
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Нужна 64-битная Windows."
    Abort
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro CHECK_RUNNING
FunctionEnd

Section "Приложение"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD}/*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Школьный день"
  CreateShortcut "$SMPROGRAMS\Школьный день\Школьный день.lnk" "$INSTDIR\bin\school-day.exe"
  CreateShortcut "$DESKTOP\Школьный день.lnk" "$INSTDIR\bin\school-day.exe"
  WriteRegStr HKCU "Software\SchoolDay" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "DisplayName" "Школьный день"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "Publisher" "sqeezy-53"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "DisplayIcon" "$INSTDIR\bin\school-day.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  RMDir /r "$INSTDIR\bin"
  RMDir /r "$INSTDIR\lib"
  RMDir /r "$INSTDIR\share"
  RMDir /r "$INSTDIR\etc"
  Delete "$INSTDIR\NOTICE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$DESKTOP\Школьный день.lnk"
  RMDir /r "$SMPROGRAMS\Школьный день"
  DeleteRegKey HKCU "Software\SchoolDay"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SchoolDay"
  ; Расписание и домашние задания находятся в пользовательской папке, её не удаляем.
SectionEnd
