; DayZ Launcher installer template (docs/09 D-067).
; Verbatim copy of Tauri's installer.nsi at tag tauri-cli-v2.11.5
;   https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
; with the changes marked below by `; >>> dzl-change:` … `; <<< dzl-change`, each one
; carrying the upstream line it replaces so the checker can put them all back and
; compare against the real thing (D-205). Today: the per-user install directory
; (D-067, S-56, Q19), an options page before the install (D-206), the flags passed to
; the uninstaller an upgrade runs and refusing to skip a locked binary (D-205), and
; skipping the reinstall page for a straight upgrade (D-207), keeping the Add/Remove
; entry through an upgrade and waiting for the old binary's lock to clear (D-214), the
; themed pages and their art (D-258), the shell's icon-cache signal after an install
; (D-260), and the Finish page's desktop shortcut taking the versioned icon (D-262,
; D-279). The installer hooks live in hooks.nsh, which this check does not cover.
; When @tauri-apps/cli is upgraded, run `node tools/nsis_template_check.js --write`,
; which re-fetches the template at the new tag and re-applies every marked change.
; --- end of DayZ Launcher header; everything below is upstream ---
Unicode true
ManifestDPIAware true
; Add in `dpiAwareness` `PerMonitorV2` to manifest for Windows 10 1607+ (note this should not affect lower versions since they should be able to ignore this and pick up `dpiAware` `true` set by `ManifestDPIAware true`)
; Currently undocumented on NSIS's website but is in the Docs folder of source tree, see
; https://github.com/kichik/nsis/blob/5fc0b87b819a9eec006df4967d08e522ddd651c9/Docs/src/attributes.but#L286-L300
; https://github.com/tauri-apps/tauri/pull/10106
ManifestDPIAwareness PerMonitorV2

!if "{{compression}}" == "none"
  SetCompress off
!else
  ; Set the compression algorithm. We default to LZMA.
  SetCompressor /SOLID "{{compression}}"
!endif

; Keep above !include to stay ahead of any plugin command
; see https://github.com/tauri-apps/tauri/pull/15422#discussion_r3289239624
{{#if signed_plugins_path}}
!addplugindir "{{signed_plugins_path}}"
{{/if}}

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "utils.nsh"
!include "FileAssociation.nsh"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "StrFunc.nsh"
${StrCase}
${StrLoc}

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define HOMEPAGE "{{homepage}}"
!define INSTALLMODE "{{install_mode}}"
!define LICENSE "{{license}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define UNINSTALLERICON "{{uninstaller_icon}}"
!define UNINSTALLERHEADERIMAGE "{{uninstaller_header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define ARCH "{{arch}}"
!define ADDITIONALPLUGINSPATH "{{additional_plugins_path}}"
!define ALLOWDOWNGRADES "{{allow_downgrades}}"
!define DISPLAYLANGUAGESELECTOR "{{display_language_selector}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define MINIMUMWEBVIEW2VERSION "{{minimum_webview2_version}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"
!define STARTMENUFOLDER "{{start_menu_folder}}"

Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var WixMode
Var OldMainBinaryName

Name "${PRODUCTNAME}"
BrandingText "${COPYRIGHT}"
OutFile "${OUTFILE}"

; We don't actually use this value as default install path,
; it's just for nsis to append the product name folder in the directory selector
; https://nsis.sourceforge.io/Reference/InstallDir
!define PLACEHOLDER_INSTALL_DIR "placeholder\${PRODUCTNAME}"
InstallDir "${PLACEHOLDER_INSTALL_DIR}"

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

# additional plugins
!addplugindir "${ADDITIONALPLUGINSPATH}"

; Uninstaller signing command
!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

; Handle install mode, `perUser`, `perMachine` or `both`
!if "${INSTALLMODE}" == "perMachine"
  RequestExecutionLevel admin
!endif

!if "${INSTALLMODE}" == "currentUser"
  RequestExecutionLevel user
!endif

!if "${INSTALLMODE}" == "both"
  !define MULTIUSER_MUI
  !define MULTIUSER_INSTALLMODE_INSTDIR "${PRODUCTNAME}"
  !define MULTIUSER_INSTALLMODE_COMMANDLINE
  !if "${ARCH}" == "x64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !else if "${ARCH}" == "arm64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !endif
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTKEY}"
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "CurrentUser"
  !define MULTIUSER_INSTALLMODEPAGE_SHOWUSERNAME
  !define MULTIUSER_INSTALLMODE_FUNCTION RestorePreviousInstallLocation
  !define MULTIUSER_EXECUTIONLEVEL Highest
  !include MultiUser.nsh
!endif

; Installer icon
!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

; Installer sidebar image
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif

; Enable header images for installer and uninstaller pages when either image is configured.
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!else if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!endif

; Installer header image
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_BITMAP "${HEADERIMAGE}"
!endif

; Uninstaller header image
!if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_UNBITMAP "${UNINSTALLERHEADERIMAGE}"
!endif

; Uninstaller icon
!if "${UNINSTALLERICON}" != ""
  !define MUI_UNICON "${UNINSTALLERICON}"
!endif

; Define registry key to store installer language
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"

; >>> dzl-change: ; Installer pages, must be ordered as they appear
; The launcher's own look on every page (D-258): its dark surfaces, light text and lime
; accent, the logo beside the welcome and finish pages, the gas mask in the header, and
; a splash for an interactive install. MUI colours what it draws (the header band, the
; welcome and finish pages); the pages it leaves to Windows are coloured from their
; SHOW callbacks (DzlDark, after the pages below). Check boxes, radio buttons, group
; boxes and the progress bar ignore colours while themed (NSIS bug #443), so on a dark
; page they are drawn classic; push buttons keep the system look. Areas are told apart
; the way the launcher does it, by surface rather than by line: the header band and the
; welcome and finish pages are its raised surface, a page body its base, and the etched
; dividers MUI draws (white on a dark window) are hidden. English only, like the
; installer: the page texts below are not in the language files.
!define DZL_BG 0F1216
!define DZL_HEAD 161B22
!define DZL_SURFACE 1B222C
!define DZL_TEXT E6E9EE
!define DZL_MUTED 8B949E
!define DZL_ACCENT A3E635
!define MUI_BGCOLOR ${DZL_HEAD}
!define MUI_TEXTCOLOR ${DZL_TEXT}
!define MUI_FORCECLASSICCONTROLS
!define MUI_HEADERIMAGE_RIGHT
!define MUI_INSTFILESPAGE_COLORS "${DZL_TEXT} ${DZL_SURFACE}"
!define MUI_CUSTOMFUNCTION_GUIINIT DzlGuiInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.DzlGuiInit
; The rest of the artwork sits beside the sidebar image Tauri was given (src-tauri/nsis).
!searchreplace DZL_ART "${SIDEBARIMAGE}" "sidebar.bmp" ""
!define MUI_WELCOMEPAGE_TITLE "Welcome to ${PRODUCTNAME}"
!define MUI_WELCOMEPAGE_TEXT "The DayZ server browser that checks every player count with the server itself, syncs the Workshop mods a server needs and gets you in with one click.$\r$\n$\r$\nThis installs version ${VERSION} for your Windows account; no administrator rights are needed.$\r$\n$\r$\nClick Next to continue."
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DzlWelcomeShow
Var DzlHwnd
Var DzlChild
Var DzlClass
Var DzlStyle
Var DzlWidth
Var DzlBitmap
; Installer pages, must be ordered as they appear
; <<< dzl-change
; 1. Welcome Page
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_WELCOME

; 2. License Page (if defined)
!if "${LICENSE}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!endif

; 3. Install mode (if it is set to `both`)
!if "${INSTALLMODE}" == "both"
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MULTIUSER_PAGE_INSTALLMODE
!endif

; 4. Custom page to ask user if he wants to reinstall/uninstall
;    only if a previous installation was detected
Var ReinstallPageCheck
Page custom PageReinstall PageLeaveReinstall
Function PageReinstall
  ; Uninstall previous WiX installation if exists.
  ;
  ; A WiX installer stores the installation info in registry
  ; using a UUID and so we have to loop through all keys under
  ; `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  ; and check if `DisplayName` and `Publisher` keys match ${PRODUCTNAME} and ${MANUFACTURER}
  ;
  ; This has a potential issue that there maybe another installation that matches
  ; our ${PRODUCTNAME} and ${MANUFACTURER} but wasn't installed by our WiX installer,
  ; however, this should be fine since the user will have to confirm the uninstallation
  ; and they can chose to abort it if doesn't make sense.
  StrCpy $0 0
  wix_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wix_loop_done ; Exit loop if there is no more keys to loop on
    IntOp $0 $0 + 1
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    StrCmp "$R0$R1" "${PRODUCTNAME}${MANUFACTURER}" 0 wix_loop
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    ${StrCase} $R1 $R0 "L"
    ${StrLoc} $R0 $R1 "msiexec" ">"
    StrCmp $R0 0 0 wix_loop_done
    StrCpy $WixMode 1
    StrCpy $R6 "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
    Goto compare_version
  wix_loop_done:

  ; Check if there is an existing installation, if not, abort the reinstall page
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" ""
  ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
  ${IfThen} "$R0$R1" == "" ${|} Abort ${|}

  ; Compare this installar version with the existing installation
  ; and modify the messages presented to the user accordingly
  compare_version:
  StrCpy $R4 "$(older)"
  ${If} $WixMode = 1
    ReadRegStr $R0 HKLM "$R6" "DisplayVersion"
  ${Else}
    ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${EndIf}
  ${IfThen} $R0 == "" ${|} StrCpy $R4 "$(unknown)" ${|}

  nsis_tauri_utils::SemverCompare "${VERSION}" $R0
  Pop $R0
  ; Reinstalling the same version
  ${If} $R0 = 0
    StrCpy $R1 "$(alreadyInstalledLong)"
    StrCpy $R2 "$(addOrReinstall)"
    StrCpy $R3 "$(uninstallApp)"
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(chooseMaintenanceOption)"
; >>> dzl-change:   ; Upgrading\n  ${ElseIf} $R0 = 1\n    StrCpy $R1 "$(olderOrUnknownVersionInstalled)"\n    StrCpy $R2 "$(uninstallBeforeInstalling)"\n    StrCpy $R3 "$(dontUninstall)"\n    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(choowHowToInstall)"
  ; An upgrade is not a question. Upstream shows a page whose pre-selected answer is
  ; "Uninstall before installing", which launches the previous version's uninstaller
  ; in the middle of the install — a second window the user did not ask for, and, on
  ; anything older than this build, its confirm page with the "Delete application
  ; data" checkbox on it. This installer overwrites in place; the uninstall achieves
  ; nothing except losing shortcuts, pins and possibly the user's data. So: skip the
  ; page for a straight upgrade, exactly as `/UPDATE` already does for the auto-updater.
  ; Same-version reinstalls and downgrades still ask, because there the answer is
  ; genuinely the user's (D-207).
  ; Upgrading
  ${ElseIf} $R0 = 1
    ${If} $WixMode <> 1
      Abort
    ${EndIf}
    StrCpy $R1 "$(olderOrUnknownVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    StrCpy $R3 "$(dontUninstall)"
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(choowHowToInstall)"
; <<< dzl-change
  ; Downgrading
  ${ElseIf} $R0 = -1
    StrCpy $R1 "$(newerVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    !if "${ALLOWDOWNGRADES}" == "true"
      StrCpy $R3 "$(dontUninstall)"
    !else
      StrCpy $R3 "$(dontUninstallDowngrade)"
    !endif
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(choowHowToInstall)"
  ${Else}
    Abort
  ${EndIf}

  ; Skip showing the page if passive
  ;
  ; Note that we don't call this earlier at the begining
  ; of this function because we need to populate some variables
  ; related to current installed version if detected and whether
  ; we are downgrading or not.
  ${If} $PassiveMode = 1
    Call PageLeaveReinstall
  ${Else}
    nsDialogs::Create 1018
    Pop $R4
    ${IfThen} $(^RTL) = 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}

    ${NSD_CreateLabel} 0 0 100% 24u $R1
    Pop $R1

    ${NSD_CreateRadioButton} 30u 50u -30u 8u $R2
    Pop $R2
    ${NSD_OnClick} $R2 PageReinstallUpdateSelection

    ${NSD_CreateRadioButton} 30u 70u -30u 8u $R3
    Pop $R3
    ; Disable this radio button if downgrading and downgrades are disabled
    !if "${ALLOWDOWNGRADES}" == "false"
      ${IfThen} $R0 = -1 ${|} EnableWindow $R3 0 ${|}
    !endif
    ${NSD_OnClick} $R3 PageReinstallUpdateSelection

    ; Check the first radio button if this the first time
    ; we enter this page or if the second button wasn't
    ; selected the last time we were on this page
    ${If} $ReinstallPageCheck <> 2
      SendMessage $R2 ${BM_SETCHECK} ${BST_CHECKED} 0
    ${Else}
      SendMessage $R3 ${BM_SETCHECK} ${BST_CHECKED} 0
    ${EndIf}

; >>> dzl-change:     ${NSD_SetFocus} $R2\n    nsDialogs::Show
    ; Dark like every other page (D-258).
    StrCpy $DzlHwnd $R4
    Call DzlDarkIn
    ${NSD_SetFocus} $R2
    nsDialogs::Show
; <<< dzl-change
  ${EndIf}
FunctionEnd
Function PageReinstallUpdateSelection
  ${NSD_GetState} $R2 $R1
  ${If} $R1 == ${BST_CHECKED}
    StrCpy $ReinstallPageCheck 1
  ${Else}
    StrCpy $ReinstallPageCheck 2
  ${EndIf}
FunctionEnd
Function PageLeaveReinstall
  ${NSD_GetState} $R2 $R1

  ; If migrating from Wix, always uninstall
  ${If} $WixMode = 1
    Goto reinst_uninstall
  ${EndIf}

  ; In update mode, always proceeds without uninstalling
  ${If} $UpdateMode = 1
    Goto reinst_done
  ${EndIf}

  ; $R0 holds whether same(0)/upgrading(1)/downgrading(-1) version
  ; $R1 holds the radio buttons state:
  ;   1 => first choice was selected
  ;   0 => second choice was selected
  ${If} $R0 = 0 ; Same version, proceed
    ${If} $R1 = 1              ; User chose to add/reinstall
      Goto reinst_done
    ${Else}                    ; User chose to uninstall
      Goto reinst_uninstall
    ${EndIf}
  ${ElseIf} $R0 = 1 ; Upgrading
    ${If} $R1 = 1              ; User chose to uninstall
      Goto reinst_uninstall
    ${Else}
      Goto reinst_done         ; User chose NOT to uninstall
    ${EndIf}
  ${ElseIf} $R0 = -1 ; Downgrading
    ${If} $R1 = 1              ; User chose to uninstall
      Goto reinst_uninstall
    ${Else}
      Goto reinst_done         ; User chose NOT to uninstall
    ${EndIf}
  ${EndIf}

  reinst_uninstall:
    HideWindow
    ClearErrors

    ${If} $WixMode = 1
      ReadRegStr $R1 HKLM "$R6" "UninstallString"
      ExecWait '$R1' $0
    ${Else}
      ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
      ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
      ; >>> dzl-change:       ${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 "$R1 /UPDATE" ${|} ; append /UPDATE
      ; Upstream appends /UPDATE only
      ; when the *installer* was given it, so a plain GUI upgrade launches the previous
      ; uninstaller with no flags at all — and the user, halfway through installing, is
      ; shown the uninstaller's own confirm page, "Delete application data" checkbox
      ; included. A tick there wipes favourites, join history, population and settings,
      ; and the same run removes the Start Menu and Desktop shortcuts and unpins them,
      ; because every one of those is gated on `$UpdateMode <> 1`.
      ;
      ; This uninstall is not the user's; it is a step inside an install. /UPDATE says
      ; so and /S stops an uninstaller window appearing in the middle of one. Fixing it
      ; on the installer side is what reaches people already on an older build: theirs
      ; is the uninstaller that runs, and it already honours /UPDATE.
      StrCpy $R1 "$R1 /UPDATE /S"
      ; <<< dzl-change
      ${IfThen} $PassiveMode = 1 ${|} StrCpy $R1 "$R1 /P" ${|} ; append /P
      StrCpy $R1 "$R1 _?=$4" ; append uninstall directory
      ExecWait '$R1' $0
    ${EndIf}

    BringToFront

    ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code

    ${If} $0 <> 0
    ${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ; User cancelled wix uninstaller? return to select un/reinstall page
      ${If} $WixMode = 1
      ${AndIf} $0 = 1602
        Abort
      ${EndIf}

      ; User cancelled NSIS uninstaller? return to select un/reinstall page
      ${If} $0 = 1
        Abort
      ${EndIf}

      ; Other erros? show generic error message and return to select un/reinstall page
      MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"
      Abort
    ${EndIf}
  reinst_done:
FunctionEnd

; 5. Choose install directory page
; >>> dzl-change: !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive\n!insertmacro MUI_PAGE_DIRECTORY
; Dark like every other page (D-258).
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DzlDark
!insertmacro MUI_PAGE_DIRECTORY
; <<< dzl-change

; 6. Start menu shortcut page
Var AppStartMenuFolder
!if "${STARTMENUFOLDER}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !define MUI_STARTMENUPAGE_DEFAULTFOLDER "${STARTMENUFOLDER}"
!else
  !define MUI_PAGE_CUSTOMFUNCTION_PRE Skip
!endif
!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder

; >>> dzl-change: ; 7. Installation page\n!insertmacro MUI_PAGE_INSTFILES
; An options page before anything is written, so a choice about what the launcher
; does is made before it has ever run (D-206). One option today; the page exists so
; the next one has somewhere to go. Skipped for silent and passive installs, which
; take /NONEWS instead.
Var OptNewsCheckbox
Var OptNewsState
Page custom PageOptions PageLeaveOptions
Function PageOptions
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "Options" "These can all be changed later in Settings."
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  StrCpy $DzlHwnd $0
  ${NSD_CreateCheckbox} 0 8u 100% 12u "Do not show the DayZ news page"
  Pop $OptNewsCheckbox
  ${If} $OptNewsState = 1
    ${NSD_Check} $OptNewsCheckbox
  ${EndIf}
  ${NSD_CreateLabel} 12u 24u 96% 34u "With the news page off, the launcher never contacts Steam's news feed, its picture CDN or YouTube. It can be turned back on at any time in Settings."
  Pop $0
  Call DzlDarkIn
  nsDialogs::Show
FunctionEnd
Function PageLeaveOptions
  ${NSD_GetState} $OptNewsCheckbox $OptNewsState
FunctionEnd
; Applied after everything is installed, so the uninstall step an upgrade runs
; cannot take the answer with it. The launcher reads this once, applies it to
; settings.json and deletes it.
Function DisableNews
  WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "DisableNews" 1
FunctionEnd

; 7. Installation page
; Dark, with a lime progress bar (D-258).
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DzlDark
!insertmacro MUI_PAGE_INSTFILES
; <<< dzl-change

; >>> dzl-change: ; 8. Finish page
; In the launcher's words and colours (D-258).
!define MUI_FINISHPAGE_TITLE "Ready to drop in"
!define MUI_FINISHPAGE_TEXT "${PRODUCTNAME} is installed. Keep Steam running while you use it: the server list and your mods come through Steam."
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DzlFinishShow
; 8. Finish page
; <<< dzl-change
;
; Don't auto jump to finish page after installation page,
; because the installation page has useful info that can be used debug any issues with the installer.
!define MUI_FINISHPAGE_NOAUTOCLOSE
; Use show readme button in the finish page as a button create a desktop shortcut
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(createDesktop)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateOrUpdateDesktopShortcut
; Show run app after installation.
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION RunMainBinary
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_FINISH

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
FunctionEnd

; Uninstaller Pages
; 1. Confirm uninstall page
Var DeleteAppDataCheckbox
Var DeleteAppDataCheckboxState
!define /ifndef WS_EX_LAYOUTRTL         0x00400000
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow
Function un.ConfirmShow ; Add add a `Delete app data` check box
  ; $1 inner dialog HWND
  ; $2 window DPI
  ; $3 style
  ; $4 x
  ; $5 y
  ; $6 width
  ; $7 height
  FindWindow $1 "#32770" "" $HWNDPARENT ; Find inner dialog
  System::Call "user32::GetDpiForWindow(p r1) i .r2"
  ${If} $(^RTL) = 1
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE} | ${WS_EX_LAYOUTRTL}"
    IntOp $4 50 * $2
  ${Else}
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE}"
    IntOp $4 0 * $2
  ${EndIf}
  IntOp $5 100 * $2
  IntOp $6 400 * $2
  IntOp $7 25 * $2
  IntOp $4 $4 / 96
  IntOp $5 $5 / 96
  IntOp $6 $6 / 96
  IntOp $7 $7 / 96
  System::Call 'user32::CreateWindowEx(i r3, w "${__NSD_CheckBox_CLASS}", w "$(deleteAppData)", i ${__NSD_CheckBox_STYLE}, i r4, i r5, i r6, i r7, p r1, i0, i0, i0) i .s'
  Pop $DeleteAppDataCheckbox
  SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
; >>> dzl-change:   SendMessage $DeleteAppDataCheckbox ${WM_SETFONT} $1 1\nFunctionEnd
  SendMessage $DeleteAppDataCheckbox ${WM_SETFONT} $1 1
  ; Dark like the installer (D-258).
  Call un.DzlDark
FunctionEnd
; <<< dzl-change
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.ConfirmLeave
Function un.ConfirmLeave
  SendMessage $DeleteAppDataCheckbox ${BM_GETCHECK} 0 0 $DeleteAppDataCheckboxState
FunctionEnd
!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!insertmacro MUI_UNPAGE_CONFIRM

; 2. Uninstalling Page
; >>> dzl-change: !insertmacro MUI_UNPAGE_INSTFILES
; Dark, with a lime progress bar (D-258).
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.DzlDark
!insertmacro MUI_UNPAGE_INSTFILES
; <<< dzl-change

; >>> dzl-change: ;Languages
; The theme's functions (D-258). Here, after the pages, because they use variables
; the pages declare. They keep to their own variables: the reinstall page holds its
; controls in $R2 and $R3 across the call.

; Each picture comes in seven sizes, one per common display scale. MUI loads a single
; bitmap and Windows stretches it to the control by dropping or doubling pixels, so
; DzlPick measures the control and the size made for that scale is loaded instead.
!macro DZL_EXTRACT_ART UN
  InitPluginsDir
  !if "${UN}" == ""
    File "/oname=$PLUGINSDIR\dzl-sidebar-100.bmp" "${DZL_ART}sidebar.bmp"
    File "/oname=$PLUGINSDIR\dzl-sidebar-125.bmp" "${DZL_ART}sidebar-125.bmp"
    File "/oname=$PLUGINSDIR\dzl-sidebar-150.bmp" "${DZL_ART}sidebar-150.bmp"
    File "/oname=$PLUGINSDIR\dzl-sidebar-175.bmp" "${DZL_ART}sidebar-175.bmp"
    File "/oname=$PLUGINSDIR\dzl-sidebar-200.bmp" "${DZL_ART}sidebar-200.bmp"
    File "/oname=$PLUGINSDIR\dzl-sidebar-250.bmp" "${DZL_ART}sidebar-250.bmp"
    File "/oname=$PLUGINSDIR\dzl-sidebar-300.bmp" "${DZL_ART}sidebar-300.bmp"
  !endif
  File "/oname=$PLUGINSDIR\dzl-header-100.bmp" "${DZL_ART}header.bmp"
  File "/oname=$PLUGINSDIR\dzl-header-125.bmp" "${DZL_ART}header-125.bmp"
  File "/oname=$PLUGINSDIR\dzl-header-150.bmp" "${DZL_ART}header-150.bmp"
  File "/oname=$PLUGINSDIR\dzl-header-175.bmp" "${DZL_ART}header-175.bmp"
  File "/oname=$PLUGINSDIR\dzl-header-200.bmp" "${DZL_ART}header-200.bmp"
  File "/oname=$PLUGINSDIR\dzl-header-250.bmp" "${DZL_ART}header-250.bmp"
  File "/oname=$PLUGINSDIR\dzl-header-300.bmp" "${DZL_ART}header-300.bmp"
!macroend

; One step of DzlPick: this scale, if its picture still covers the control ($1 spare).
!macro _DZL_TRY SCALE
  IntOp $1 $DzlStyle * ${SCALE}
  IntOp $1 $1 / 100
  IntOp $1 $1 + 2
  ${If} $1 >= $DzlWidth
    StrCpy $DzlClass ${SCALE}
  ${EndIf}
!macroend

!macro DZL_FUNCTIONS UN
  ; In: $DzlHwnd the picture control, $DzlStyle the picture's width at 100 %.
  ; Out: $DzlClass, the smallest scale whose picture covers the control.
  Function ${UN}DzlPick
    Push $0
    Push $1
    System::Call '*(i, i, i, i) p.r1'
    System::Call 'user32::GetClientRect(p $DzlHwnd, p r1)'
    System::Call '*$1(i, i, i .r0, i)'
    System::Free $1
    StrCpy $DzlWidth $0
    StrCpy $DzlClass 300
    !insertmacro _DZL_TRY 250
    !insertmacro _DZL_TRY 200
    !insertmacro _DZL_TRY 175
    !insertmacro _DZL_TRY 150
    !insertmacro _DZL_TRY 125
    !insertmacro _DZL_TRY 100
    Pop $1
    Pop $0
  FunctionEnd

  ; The window around the pages: its background, the branding line, the header's
  ; picture at the display scale's own size, the page title in the accent.
  Function ${UN}DzlGuiInit
    !insertmacro DZL_EXTRACT_ART "${UN}"
    SetCtlColors $HWNDPARENT "" ${DZL_BG}
    SetCtlColors $mui.Branding.Text ${DZL_MUTED} ${DZL_BG}
    SetCtlColors $mui.Branding.Background "" ${DZL_BG}
    SetCtlColors $mui.Header.Text ${DZL_ACCENT} ${DZL_HEAD}
    SetCtlColors $mui.Header.SubText ${DZL_TEXT} ${DZL_HEAD}
    GetDlgItem $DzlHwnd $HWNDPARENT 1046
    StrCpy $DzlStyle 150
    Call ${UN}DzlPick
    SetBrandingImage /IMGID=1046 /RESIZETOFIT "$PLUGINSDIR\dzl-header-$DzlClass.bmp"
  FunctionEnd

  ; The SHOW callback of a page MUI builds: by then the page is the window's dialog.
  Function ${UN}DzlDark
    FindWindow $DzlHwnd "#32770" "" $HWNDPARENT
    Call ${UN}DzlDarkIn
  FunctionEnd

  ; The page dialog in $DzlHwnd and every static, edit, check box, radio button, group
  ; box and progress bar on it; and no divider under the header.
  Function ${UN}DzlDarkIn
    ShowWindow $mui.Line.Standard ${SW_HIDE}
    SetCtlColors $DzlHwnd ${DZL_TEXT} ${DZL_BG}
    StrCpy $DzlChild 0
    ${Do}
      FindWindow $DzlChild "" "" $DzlHwnd $DzlChild
      ${If} $DzlChild = 0
        ${Break}
      ${EndIf}
      System::Call 'user32::GetClassName(p $DzlChild, t .s, i 64)'
      Pop $DzlClass
      ${If} $DzlClass == "Button"
        System::Call 'user32::GetWindowLong(p $DzlChild, i -16) i .s'
        Pop $DzlStyle
        IntOp $DzlStyle $DzlStyle & 0xF
        ; 0 and 1 are push buttons, 8 is owner-drawn; the rest check, choose or group.
        ${If} $DzlStyle >= 2
        ${AndIf} $DzlStyle <= 9
        ${AndIf} $DzlStyle <> 8
          System::Call 'uxtheme::SetWindowTheme(p $DzlChild, w " ", w " ")'
          SetCtlColors $DzlChild ${DZL_TEXT} ${DZL_BG}
        ${EndIf}
      ${ElseIf} $DzlClass == "Static"
        SetCtlColors $DzlChild ${DZL_TEXT} ${DZL_BG}
      ${ElseIf} $DzlClass == "Edit"
        SetCtlColors $DzlChild ${DZL_TEXT} ${DZL_SURFACE}
      ${ElseIf} $DzlClass == "msctls_progress32"
        System::Call 'uxtheme::SetWindowTheme(p $DzlChild, w " ", w " ")'
        ; COLORREF is 0x00BBGGRR: lime A3E635 on the surface 1B222C.
        SendMessage $DzlChild ${PBM_SETBARCOLOR} 0 0x0035E6A3
        SendMessage $DzlChild ${PBM_SETBKCOLOR} 0 0x002C221B
      ${EndIf}
    ${Loop}
  FunctionEnd
!macroend
!insertmacro DZL_FUNCTIONS ""
!insertmacro DZL_FUNCTIONS "un."

; The welcome and finish pages: the logo at the display scale's size, title in lime.
!macro DZL_SIDEBAR IMAGE BITMAP TITLE
  StrCpy $DzlHwnd ${IMAGE}
  StrCpy $DzlStyle 164
  Call DzlPick
  ${NSD_SetStretchedImage} ${IMAGE} "$PLUGINSDIR\dzl-sidebar-$DzlClass.bmp" $DzlBitmap
  ${NSD_FreeImage} ${BITMAP}
  StrCpy ${BITMAP} $DzlBitmap
  SetCtlColors ${TITLE} ${DZL_ACCENT} ${DZL_HEAD}
  ShowWindow $mui.Line.FullWindow ${SW_HIDE}
!macroend
Function DzlWelcomeShow
  !insertmacro DZL_SIDEBAR $mui.WelcomePage.Image $mui.WelcomePage.Image.Bitmap $mui.WelcomePage.Title
FunctionEnd
Function DzlFinishShow
  !insertmacro DZL_SIDEBAR $mui.FinishPage.Image $mui.FinishPage.Image.Bitmap $mui.FinishPage.Title
FunctionEnd

;Languages
; <<< dzl-change
{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
{{#each language_files}}
  !include "{{this}}"
{{/each}}

Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/NS" $NoShortcutMode
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}

; >>> dzl-change:   ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode\n  ${IfNot} ${Errors}\n    StrCpy $UpdateMode 1\n  ${EndIf}
  ; /NONEWS is the silent-install equivalent of the options page (D-206). It only
  ; records the answer; the write happens from NSIS_HOOK_POSTINSTALL, after the
  ; uninstall step an upgrade runs, which would otherwise delete the key underneath it.
  ${GetOptions} $CMDLINE "/NONEWS" $0
  ${IfNot} ${Errors}
    StrCpy $OptNewsState 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  ; The logo for a moment before the first page (D-258). Never for a silent, passive
  ; or update run: the updater should not flash a picture across someone's game.
  ${IfNot} ${Silent}
  ${AndIf} $PassiveMode <> 1
  ${AndIf} $UpdateMode <> 1
    InitPluginsDir
    File "/oname=$PLUGINSDIR\dzl-splash.bmp" "${DZL_ART}splash.bmp"
    advsplash::show 900 250 300 0xFF00FF "$PLUGINSDIR\dzl-splash"
    Pop $0
  ${EndIf}
; <<< dzl-change

  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    !insertmacro MUI_LANGDLL_DISPLAY
  !endif

  !insertmacro SetContext

  ${If} $INSTDIR == "${PLACEHOLDER_INSTALL_DIR}"
    ; Set default install location
    !if "${INSTALLMODE}" == "perMachine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    !else if "${INSTALLMODE}" == "currentUser"
      ; >>> dzl-change:       StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
      ; Per-user installs go under Programs, because that is where a program belongs
      ; and the stock default collided with the official DayZ Launcher's data folder
      ; under this app's original name (D-067).
      StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
      ; <<< dzl-change
    !endif

    Call RestorePreviousInstallLocation
  ${EndIf}


  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_INIT
  !endif
FunctionEnd


Section EarlyChecks
  ; Abort silent installer if downgrades is disabled
  !if "${ALLOWDOWNGRADES}" == "false"
  ${If} ${Silent}
    ; If downgrading
    ${If} $R0 = -1
      System::Call 'kernel32::AttachConsole(i -1)i.r0'
      ${If} $0 <> 0
        System::Call 'kernel32::GetStdHandle(i -11)i.r0'
        System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
        FileWrite $0 "$(silentDowngrades)"
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
  !endif

SectionEnd

Section WebView2
  ; Check if Webview2 is already installed and skip this section
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 == ""
    ReadRegStr $4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}

  ${If} $4 == ""
    ; Webview2 installation
    ;
    ; Skip if updating
    ${If} $UpdateMode <> 1
      !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        DetailPrint "$(webview2Downloading)"
        NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Pop $0
        ${If} $0 == "success"
          DetailPrint "$(webview2DownloadSuccess)"
        ${Else}
          DetailPrint "$(webview2DownloadError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
        Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        Goto install_webview2
      !endif

      Goto webview2_done

      install_webview2:
        DetailPrint "$(installingWebview2)"
        ; $6 holds the path to the webview2 installer
        ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1
        ${If} $1 = 0
          DetailPrint "$(webview2InstallSuccess)"
        ${Else}
          DetailPrint "$(webview2InstallError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
      webview2_done:
    ${EndIf}
  ${Else}
    !if "${MINIMUMWEBVIEW2VERSION}" != ""
      ${VersionCompare} "${MINIMUMWEBVIEW2VERSION}" "$4" $R0
      ${If} $R0 = 1
        update_webview:
          DetailPrint "$(installingWebview2)"
          ${If} ${RunningX64}
            ReadRegStr $R1 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate" "path"
          ${Else}
            ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 == ""
            ReadRegStr $R1 HKCU "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 != ""
            ; Chromium updater docs: https://source.chromium.org/chromium/chromium/src/+/main:docs/updater/user_manual.md
            ; Modified from "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft EdgeWebView\ModifyPath"
            ExecWait `"$R1" /install appguid=${WEBVIEW2APPGUID}&needsadmin=true` $1
            ${If} $1 = 0
              DetailPrint "$(webview2InstallSuccess)"
            ${Else}
              MessageBox MB_ICONEXCLAMATION|MB_ABORTRETRYIGNORE "$(webview2InstallError)" IDIGNORE ignore IDRETRY update_webview
              Quit
              ignore:
            ${EndIf}
          ${EndIf}
      ${EndIf}
    !endif
  ${EndIf}
SectionEnd

; >>> dzl-change: Section Install
; A locked binary must fail the install,
; not be skipped: with NSIS's default `AllowSkipFiles on` the internal
; Abort/Retry/Ignore box takes its silent default, so `/S` exited 0 with the old exe
; still in place and the new version written to Add/Remove Programs. WebView2's child
; processes outlive the host long enough to hold it (D-196), so an auto-update could
; report success and change nothing, then offer the same update for ever.
AllowSkipFiles off
Section Install
; <<< dzl-change
  SetOutPath $INSTDIR

  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !endif

; >>> dzl-change:   !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"\n\n  ; Copy main executable\n  File "${MAINBINARYSRCPATH}"
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; CheckIfAppIsRunning kills the host and waits a flat 500 ms, but WebView2 spawns
  ; children that outlive it and keep a handle on the install folder (D-196) — the
  ; smoke test had to learn the same thing. Since AllowSkipFiles was turned off
  ; (D-205) a locked binary no longer silently skips: the install aborts, so an
  ; auto-update arriving a second too early now fails visibly. Wait for the lock to
  ; clear instead of guessing at 500 ms, up to ten seconds, and test the one thing
  ; that actually matters by trying to open the file for writing. Nothing is killed:
  ; every msedgewebview2.exe on the machine has the same image name, and most of them
  ; belong to other applications (D-214).
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R9 0
    wait_unlock_loop:
      ClearErrors
      FileOpen $R8 "$INSTDIR\${MAINBINARYNAME}.exe" a
      ${IfNot} ${Errors}
        FileClose $R8
        Goto wait_unlock_done
      ${EndIf}
      IntOp $R9 $R9 + 1
      ${If} $R9 >= 40
        Goto wait_unlock_done
      ${EndIf}
      Sleep 250
      Goto wait_unlock_loop
    wait_unlock_done:
  ${EndIf}

  ; Copy main executable
  File "${MAINBINARYSRCPATH}"
; <<< dzl-change

  ; Copy resources
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  {{/each}}

  ; Copy external binaries
  {{#each binaries}}
    File /a "/oname={{this}}" "{{no-escape @key}}"
  {{/each}}

  ; Create file associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
       !insertmacro APP_ASSOCIATE "{{ext}}" "{{or association.name ext}}" "{{association-description association.description ext}}" "$INSTDIR\${MAINBINARYNAME}.exe,0" "Open with ${PRODUCTNAME}" "$INSTDIR\${MAINBINARYNAME}.exe $\"%1$\""
    {{/each}}
  {{/each}}

  ; Register deep links
  {{#each deep_link_protocols as |protocol| ~}}
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "URL Protocol" ""
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "" "URL:${BUNDLEID} protocol"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  {{/each}}

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Save $INSTDIR in registry for future installations
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR

  !if "${INSTALLMODE}" == "both"
    ; Save install mode to be selected by default for the next installation such as updating
    ; or when uninstalling
    WriteRegStr SHCTX "${UNINSTKEY}" $MultiUser.InstallMode 1
  !endif

  ; Remove old main binary if it doesn't match new main binary name
  ReadRegStr $OldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  ${If} $OldMainBinaryName != ""
  ${AndIf} $OldMainBinaryName != "${MAINBINARYNAME}.exe"
    Delete "$INSTDIR\$OldMainBinaryName"
  ${EndIf}

  ; Save current MAINBINARYNAME for future updates
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"

  ; Registry information for add/remove programs
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"

  ${GetSize} "$INSTDIR" "/M=uninstall.exe /S=0K /G=0" $0 $1 $2
  IntOp $0 $0 + ${ESTIMATEDSIZE}
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"

  !if "${HOMEPAGE}" != ""
    WriteRegStr SHCTX "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "URLUpdateInfo" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "HelpLink" "${HOMEPAGE}"
  !endif

  ; Create start menu shortcut
  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
    Call CreateOrUpdateStartMenuShortcut
  !insertmacro MUI_STARTMENU_WRITE_END

  ; Create desktop shortcut for silent and passive installers
  ; because finish page will be skipped
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}

; >>> dzl-change:   !ifmacrodef NSIS_HOOK_POSTINSTALL
  ; Explorer keeps the icons it has drawn for a shortcut in its icon cache, keyed by
  ; the target's path, so after an in-place update the Start menu and desktop
  ; shortcuts kept the previous icon (D-260). SHCNE_ASSOCCHANGED is the documented
  ; way to tell it that cached icons are stale; SHCNF_IDLIST (0) is the flag the
  ; documentation requires with it. On its own it did not clear the old picture on the
  ; user's machine; the shortcuts now point at a versioned copy of the icon instead
  ; (hooks.nsh, D-262), and this stays as the general "cached icons are stale" signal.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  !ifmacrodef NSIS_HOOK_POSTINSTALL
; <<< dzl-change
    !insertmacro NSIS_HOOK_POSTINSTALL
  !endif

  ; Auto close this page for passive mode
  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function .onInstSuccess
  ; Check for `/R` flag only in silent and passive installers because
  ; GUI installer has a toggle for the user to (re)start the app
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $R0
    ${IfNot} ${Errors}
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "$R0"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro SetContext

  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_UNINIT
  !endif

  !insertmacro MUI_UNGETLANGUAGE

  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Section Uninstall

  !ifmacrodef NSIS_HOOK_PREUNINSTALL
    !insertmacro NSIS_HOOK_PREUNINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Delete the app directory and its content from disk
  ; Copy main executable
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Delete resources
  {{#each resources}}
    Delete "$INSTDIR\\{{this.[1]}}"
  {{/each}}

  ; Delete external binaries
  {{#each binaries}}
    Delete "$INSTDIR\\{{this}}"
  {{/each}}

  ; Delete app associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
      !insertmacro APP_UNASSOCIATE "{{ext}}" "{{or association.name ext}}"
    {{/each}}
  {{/each}}

  ; Delete deep links
  {{#each deep_link_protocols as |protocol| ~}}
    ReadRegStr $R7 SHCTX "Software\Classes\\{{protocol}}\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\\{{protocol}}"
    ${EndIf}
  {{/each}}


  ; Delete uninstaller
  Delete "$INSTDIR\uninstall.exe"

  {{#each resources_ancestors}}
  RMDir /REBOOTOK "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  ; Remove shortcuts if not updating
  ${If} $UpdateMode <> 1
    !insertmacro DeleteAppUserModelId

    ; Remove start menu shortcut
    !insertmacro MUI_STARTMENU_GETFOLDER Application $AppStartMenuFolder
    !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      RMDir "$SMPROGRAMS\$AppStartMenuFolder"
    ${EndIf}
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}

    ; Remove desktop shortcuts
    !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$DESKTOP\${PRODUCTNAME}.lnk"
      Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}

; >>> dzl-change:   ; Remove registry information for add/remove programs\n  !if "${INSTALLMODE}" == "both"\n    DeleteRegKey SHCTX "${UNINSTKEY}"\n  !else if "${INSTALLMODE}" == "perMachine"\n    DeleteRegKey HKLM "${UNINSTKEY}"\n  !else\n    DeleteRegKey HKCU "${UNINSTKEY}"\n  !endif
  ; Remove registry information for add/remove programs.
  ; Gated on $UpdateMode like the shortcuts above it; upstream leaves it ungated. An
  ; upgrade runs this uninstaller partway through the install, and between here and
  ; the install writing DisplayName back, a Cancel, a crash or a power cut left the
  ; machine with no app, no Add/Remove entry and shortcuts pointing at a deleted exe,
  ; recoverable only by downloading the installer again (D-214). Under /UPDATE the
  ; installer rewrites every one of these values a few lines later, so keeping the
  ; key leaks nothing.
  ${If} $UpdateMode <> 1
    !if "${INSTALLMODE}" == "both"
      DeleteRegKey SHCTX "${UNINSTKEY}"
    !else if "${INSTALLMODE}" == "perMachine"
      DeleteRegKey HKLM "${UNINSTKEY}"
    !else
      DeleteRegKey HKCU "${UNINSTKEY}"
    !endif
  ${EndIf}
; <<< dzl-change

  ; Removes the Autostart entry for ${PRODUCTNAME} from the HKCU Run key if it exists.
  ; This ensures the program does not launch automatically after uninstallation if it exists.
  ; If it doesn't exist, it does nothing.
  ; We do this when not updating (to preserve the registry value on updates)
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  ${EndIf}

  ; Delete app data if the checkbox is selected
  ; and if not updating
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Clear the install location $INSTDIR from registry
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"

    ; Clear the install language from registry
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"

    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTUNINSTALL
    !insertmacro NSIS_HOOK_POSTUNINSTALL
  !endif

  ; Auto close if passive mode or updating
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function RestorePreviousInstallLocation
  ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $4 "" +2 0
    StrCpy $INSTDIR $4
FunctionEnd

Function Skip
  Abort
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd
Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd

Function CreateOrUpdateStartMenuShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  StrCpy $R0 0

  !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  ${If} $R0 = 1
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  !if "${STARTMENUFOLDER}" != ""
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
FunctionEnd

Function CreateOrUpdateDesktopShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
; >>> dzl-change:   !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
  !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
  ; The Finish page makes this shortcut after NSIS_HOOK_POSTINSTALL has written the
  ; versioned icon copy and pointed the other shortcuts at it; this one takes it too,
  ; or it would show whatever Explorer cached for the exe path (hooks.nsh, D-262).
  ; A silent or passive install makes it before the hook, when the copy does not exist
  ; yet; the hook points it there a moment later (D-279).
  ${If} ${FileExists} "$INSTDIR\icons\${VERSION}.ico"
    !insertmacro DzlSetShortcutIcon "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\icons\${VERSION}.ico"
  ${EndIf}
; <<< dzl-change
FunctionEnd
