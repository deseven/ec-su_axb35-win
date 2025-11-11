; ec-su_axb35-win Installer

!define PRODUCT_NAME "ec-su_axb35-win"
!define PRODUCT_VERSION "1.2.1"
!define PRODUCT_PUBLISHER "deseven"
!define PRODUCT_WEB_SITE "https://github.com/deseven/ec-su_axb35-win"
!define PRODUCT_DIR_REGKEY "Software\Microsoft\Windows\CurrentVersion\App Paths\ec-su_axb35-server.exe"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

!define SERVICE_NAME "ec-su_axb35-win"
!define SERVICE_DISPLAY_NAME "EC SU_AXB35 Server"
!define SERVICE_DESCRIPTION "Control server for SU_AXB35 Embedded Controller"

; Installation directories
!define INSTALL_DIR "$PROGRAMFILES64\ec-su_axb35-win"
!define WINRING_DIR "$APPDATA\ec-su_axb35-win\winring0"

; Include required headers
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WinMessages.nsh"

; MUI Settings
!define MUI_ABORTWARNING
!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

; Welcome page
!insertmacro MUI_PAGE_WELCOME
; License page (optional - uncomment if you have a license file)
; !insertmacro MUI_PAGE_LICENSE "license.txt"
; Directory page
!insertmacro MUI_PAGE_DIRECTORY
; Instfiles page
!insertmacro MUI_PAGE_INSTFILES
; Finish page with option to run client
!define MUI_FINISHPAGE_RUN "$INSTDIR\ec-su_axb35-win-client.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Run EC SU_AXB35 Client"
!insertmacro MUI_PAGE_FINISH

; Uninstaller pages
!insertmacro MUI_UNPAGE_INSTFILES

; Language files
!insertmacro MUI_LANGUAGE "English"

; Reserve files (MUI_RESERVEFILE_INSTALLOPTIONS is deprecated in MUI2)
; Use ReserveFile if needed for specific plugins

; MUI end ------

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "ec-su_axb35-win-installer-${PRODUCT_VERSION}.exe"
InstallDir "${INSTALL_DIR}"
InstallDirRegKey HKLM "${PRODUCT_DIR_REGKEY}" ""
ShowInstDetails show
ShowUnInstDetails show
RequestExecutionLevel admin

; Version Information
VIProductVersion "1.0.0.0"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "Comments" "EC SU_AXB35 WIN Installer"
VIAddVersionKey "CompanyName" "${PRODUCT_PUBLISHER}"
VIAddVersionKey "LegalTrademarks" ""
VIAddVersionKey "LegalCopyright" "© ${PRODUCT_PUBLISHER}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} Installer"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"

Function .onInit
  ; This is important to have $APPDATA variable
  ; point to ProgramData folder instead of current user's Roaming folder
  SetShellVarContext all
  
  ; Check if running as administrator
  UserInfo::GetAccountType
  pop $0
  ${If} $0 != "admin"
    MessageBox MB_ICONSTOP "Administrator rights required!"
    SetErrorLevel 740 ; ERROR_ELEVATION_REQUIRED
    Quit
  ${EndIf}
FunctionEnd

; Service management functions
Function StopExistingService
  DetailPrint "Checking for existing service..."
  
  ; Stop the service if it's running
  nsExec::ExecToLog 'sc query "${SERVICE_NAME}"'
  Pop $0
  ${If} $0 == 0
    DetailPrint "Stopping existing service..."
    nsExec::ExecToLog 'sc stop "${SERVICE_NAME}"'
    Sleep 3000 ; Wait 3 seconds for service to stop
  ${EndIf}
FunctionEnd

Function KillExistingClientProcess
  DetailPrint "Checking for existing client process..."
  
  ; Kill any running client processes
  nsExec::ExecToLog 'taskkill /F /IM ec-su_axb35-win-client.exe'
  Pop $0
  ${If} $0 == 0
    DetailPrint "Existing client process terminated"
    Sleep 1000 ; Wait 1 second
  ${EndIf}
FunctionEnd

Function CreateOrUpdateAndStartService
  DetailPrint "Creating or updating service..."
  
  ; Try to create the service first
  nsExec::ExecToLog 'sc create "${SERVICE_NAME}" binPath= "$INSTDIR\ec-su_axb35-server.exe --service" DisplayName= "${SERVICE_DISPLAY_NAME}" start= auto'
  Pop $0
  ${If} $0 == 1073
    ; Service already exists, update it instead
    DetailPrint "Service exists, updating configuration..."
    nsExec::ExecToLog 'sc config "${SERVICE_NAME}" binPath= "$INSTDIR\ec-su_axb35-server.exe --service" DisplayName= "${SERVICE_DISPLAY_NAME}" start= auto'
    Pop $0
    ${If} $0 != 0
      MessageBox MB_ICONSTOP "Failed to update service configuration. Error code: $0"
      Abort
    ${EndIf}
  ${ElseIf} $0 != 0
    MessageBox MB_ICONSTOP "Failed to create service. Error code: $0"
    Abort
  ${EndIf}
  
  ; Set service description
  nsExec::ExecToLog 'sc description "${SERVICE_NAME}" "${SERVICE_DESCRIPTION}"'
  
  DetailPrint "Starting service..."
  nsExec::ExecToLog 'sc start "${SERVICE_NAME}"'
  Pop $0
  ${If} $0 != 0
    ${If} $0 != 1056
      ; Error 1056 means service is already running, which is fine
      MessageBox MB_ICONEXCLAMATION "Service created/updated but failed to start. You can start it manually from Services. Error code: $0"
    ${Else}
      DetailPrint "Service is already running!"
    ${EndIf}
  ${Else}
    DetailPrint "Service started successfully!"
  ${EndIf}
FunctionEnd


Section "MainSection" SEC01
  ; Stop existing service if running
  Call StopExistingService
  
  ; Kill existing client process if running
  Call KillExistingClientProcess
  
  ; Create installation directory
  SetOutPath "$INSTDIR"
  SetOverwrite ifnewer
  
  ; Install server binary
  DetailPrint "Installing server binary..."
  File "server\target\release\ec-su_axb35-server.exe"
  
  ; Install client binary
  DetailPrint "Installing client binary..."
  File "client\target\release\ec-su_axb35-win-client.exe"
  
  ; Create winring0 directory and install files
  DetailPrint "Installing WinRing0 drivers..."
  CreateDirectory "$APPDATA\ec-su_axb35-win\winring0"
  SetOutPath "$APPDATA\ec-su_axb35-win\winring0"
  File "server\winring0\WinRing0.sys"
  File "server\winring0\WinRing0x64.sys"
  
  ; Create scripts directory and install scripts
  DetailPrint "Installing scripts..."
  CreateDirectory "$APPDATA\ec-su_axb35-win\scripts"
  SetOutPath "$APPDATA\ec-su_axb35-win\scripts"
  File "server\scripts\info.ps1"
  File "server\scripts\test_fan_mode_fixed.ps1"
  
  ; Set proper permissions for winring0 directory using built-in commands
  ; The service will run as SYSTEM and should have access to PROGRAMDATA
  DetailPrint "Setting directory permissions..."
  
  ; Create/update and start service
  Call CreateOrUpdateAndStartService
SectionEnd

Section -AdditionalIcons
  SetOutPath $INSTDIR
  WriteIniStr "$INSTDIR\${PRODUCT_NAME}.url" "InternetShortcut" "URL" "${PRODUCT_WEB_SITE}"
  CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\EC SU_AXB35 Client.lnk" "$INSTDIR\ec-su_axb35-win-client.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\Website.lnk" "$INSTDIR\${PRODUCT_NAME}.url"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk" "$INSTDIR\uninst.exe"
SectionEnd

Section -Post
  WriteUninstaller "$INSTDIR\uninst.exe"
  WriteRegStr HKLM "${PRODUCT_DIR_REGKEY}" "" "$INSTDIR\ec-su_axb35-server.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "$(^Name)"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninst.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\ec-su_axb35-server.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
SectionEnd

Function un.onUninstSuccess
  HideWindow
  MessageBox MB_ICONINFORMATION|MB_OK "$(^Name) was successfully removed from your computer."
FunctionEnd

Function un.onInit
  ; This is important to have $APPDATA variable
  ; point to ProgramData folder instead of current user's Roaming folder
  SetShellVarContext all
  
  ; Check if running as administrator
  UserInfo::GetAccountType
  pop $0
  ${If} $0 != "admin"
    MessageBox MB_ICONSTOP "Administrator rights required!"
    SetErrorLevel 740 ; ERROR_ELEVATION_REQUIRED
    Quit
  ${EndIf}
  
  MessageBox MB_ICONQUESTION|MB_YESNO|MB_DEFBUTTON2 "Are you sure you want to completely remove $(^Name) and all of its components?" IDYES +2
  Abort
FunctionEnd

Section Uninstall
  ; Stop and remove service
  DetailPrint "Stopping and removing service..."
  nsExec::ExecToLog 'sc stop "${SERVICE_NAME}"'
  Sleep 3000
  nsExec::ExecToLog 'sc delete "${SERVICE_NAME}"'
  Sleep 1000
  
  ; Kill client process if running before uninstall
  nsExec::ExecToLog 'taskkill /F /IM ec-su_axb35-win-client.exe'
  Sleep 1000
  
  ; Remove files
  Delete "$INSTDIR\${PRODUCT_NAME}.url"
  Delete "$INSTDIR\uninst.exe"
  Delete "$INSTDIR\ec-su_axb35-server.exe"
  Delete "$INSTDIR\ec-su_axb35-win-client.exe"
  
  ; Remove winring0 files
  Delete "$APPDATA\ec-su_axb35-win\winring0\WinRing0.sys"
  Delete "$APPDATA\ec-su_axb35-win\winring0\WinRing0x64.sys"
  RMDir "$APPDATA\ec-su_axb35-win\winring0"
  
  ; Remove scripts files
  Delete "$APPDATA\ec-su_axb35-win\scripts\info.ps1"
  Delete "$APPDATA\ec-su_axb35-win\scripts\test_fan_mode_fixed.ps1"
  RMDir "$APPDATA\ec-su_axb35-win\scripts"
  
  RMDir "$APPDATA\ec-su_axb35-win"
  
  ; Remove shortcuts
  Delete "$SMPROGRAMS\${PRODUCT_NAME}\EC SU_AXB35 Client.lnk"
  Delete "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk"
  Delete "$SMPROGRAMS\${PRODUCT_NAME}\Website.lnk"
  RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
  
  ; Remove installation directory
  RMDir "$INSTDIR"
  
  ; Remove registry keys
  DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"
  DeleteRegKey HKLM "${PRODUCT_DIR_REGKEY}"
  SetAutoClose true
SectionEnd