; Installer hooks (D-206). Tauri's supported extension point — `installerHooks` in
; tauri.conf.json — so nothing here counts as a deviation from the stock template and
; `nsis_template_check.js` has nothing to say about it.
;
; This file is included before the template defines VERSION, PRODUCTNAME and the rest,
; so everything that uses them is a macro: macros expand where they are inserted, by
; which point the defines exist.

; The version this setup replaces, read before the install rewrites it (D-279).
Var DzlPrevVersion

; ---------------------------------------------------------------------------------------
; Shortcut icons that follow an update (D-262).
;
; Explorer keeps the pictures it has drawn in its icon cache, keyed by where an icon
; comes from, and every shortcut Tauri writes takes its icon from the exe — whose path
; never changes. So the desktop and Start menu shortcuts and a pinned taskbar button kept
; the previous version's icon through an update, and even through an uninstall and a
; reinstall into the same folder. SHCNE_ASSOCCHANGED (D-260) did not clear it on the
; user's machine. A copy of the icon named after the version gives each release a
; location the cache has never seen, so there is nothing stale to find.

; Points one shortcut's icon at `icon`, keeping everything else the link holds (target,
; arguments, the AppUserModelID Tauri sets), then tells the shell to re-read that link.
!macro DzlSetShortcutIcon shortcut icon
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READWRITE})'
      ${IShellLink::SetIconLocation} $0 '("${icon}", 0)'
      ${IPersistFile::Save} $1 '("${shortcut}",1)'
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  ; SHCNE_UPDATEITEM (0x2000) with SHCNF_PATHW (0x5): this item changed; re-read it.
  System::Call 'shell32::SHChangeNotify(i 0x2000, i 0x5, w "${shortcut}", p 0)'
!macroend

; Pushes 1 when `shortcut` points at `exe`, else 0. The path is read expanded (flags 0,
; where Tauri's IsShortcutTarget reads it raw), so a link stored with an environment
; variable in it still matches; the comparison ignores case. `GetPath` fills its own
; buffer; the 520 bytes allocated here before were never used and never freed (D-279).
; Uses $0–$3.
!macro DzlIsShortcutTo shortcut exe
  StrCpy $3 0
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}", .r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READ})'
      ${IShellLink::GetPath} $0 '(.r2, 260, 0, 0)'
      ${If} $2 == "${exe}"
        StrCpy $3 1
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  Push $3
!macroend

!macro DzlIsOurShortcut shortcut
  !insertmacro DzlIsShortcutTo "${shortcut}" "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend

; Only a link that points at this install is touched: a user's own shortcut to
; something else, or to another copy of the launcher, is left as it is.
!macro DzlRefreshIconIfOurs shortcut
  ${If} ${FileExists} "${shortcut}"
    !insertmacro DzlIsOurShortcut "${shortcut}"
    Pop $0
    ${If} $0 = 1
      !insertmacro DzlSetShortcutIcon "${shortcut}" "$INSTDIR\icons\${VERSION}.ico"
    ${EndIf}
  ${EndIf}
!macroend

; ---------------------------------------------------------------------------------------
; Installs from before the rename (D-279).
;
; Up to v0.1.21 the product was "DayZ Launcher", with its own folder, Add/Remove entry
; and shortcuts. Its updater still finds our releases, and runs this setup in update
; mode, which looks only under the new name: it installed a second copy and made no
; shortcut, so every start from the old shortcuts ran the old build, which offered the
; same update again — and both builds share one data folder. After a successful
; install, such a copy is retired: its shortcuts are pointed at this install, then its
; own uninstaller removes its files in update mode, which keeps shortcuts and data and
; deletes its Add/Remove entry.

; Points one shortcut at this install's exe, working folder and icon.
!macro DzlSetShortcutTarget shortcut
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READWRITE})'
      ${IShellLink::SetPath} $0 '("$INSTDIR\${MAINBINARYNAME}.exe")'
      ${IShellLink::SetWorkingDirectory} $0 '("$INSTDIR")'
      ${IShellLink::SetIconLocation} $0 '("$INSTDIR\icons\${VERSION}.ico", 0)'
      ${IPersistFile::Save} $1 '("${shortcut}",1)'
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x2000, i 0x5, w "${shortcut}", p 0)'
!macroend

!macro DzlRetargetIfTo shortcut exe
  ${If} ${FileExists} "${shortcut}"
    !insertmacro DzlIsShortcutTo "${shortcut}" "${exe}"
    Pop $0
    ${If} $0 = 1
      !insertmacro DzlSetShortcutTarget "${shortcut}"
    ${EndIf}
  ${EndIf}
!macroend

; `uninstRoot` and the three folders are parameters so the macro can be tested against a
; scratch key and scratch folders; the install passes the real ones. Uses $0–$3, $R0–$R4.
!macro DzlRetireOldProduct oldName uninstRoot desktopDir smDir pinnedDir
  ReadRegStr $R0 HKCU "${uninstRoot}\${oldName}" "Publisher"
  ReadRegStr $R1 HKCU "${uninstRoot}\${oldName}" "InstallLocation"
  ; Written quoted by the template ("$\"$INSTDIR$\"").
  StrCpy $R2 $R1 1
  ${If} $R2 == '"'
    StrCpy $R1 $R1 "" 1
    StrCpy $R1 $R1 -1
  ${EndIf}
  ${If} $R0 == "${MANUFACTURER}"
  ${AndIf} $R1 != ""
    ${If} $R1 == $INSTDIR
      ; This install went into the old folder: the old entry now describes this one's
      ; files and would uninstall them, so only the stale registry goes.
      DeleteRegKey HKCU "${uninstRoot}\${oldName}"
    ${ElseIf} ${FileExists} "$R1\uninstall.exe"
      !insertmacro DzlRetargetIfTo "${desktopDir}\${oldName}.lnk" "$R1\${MAINBINARYNAME}.exe"
      !insertmacro DzlRetargetIfTo "${smDir}\${oldName}.lnk" "$R1\${MAINBINARYNAME}.exe"
      !insertmacro DzlRetargetIfTo "${smDir}\${oldName}\${oldName}.lnk" "$R1\${MAINBINARYNAME}.exe"
      FindFirst $R3 $R4 "${pinnedDir}\*.lnk"
      ${DoWhile} $R4 != ""
        !insertmacro DzlRetargetIfTo "${pinnedDir}\$R4" "$R1\${MAINBINARYNAME}.exe"
        FindNext $R3 $R4
      ${Loop}
      FindClose $R3
      ; In place (`_?=`, which must come last and unquoted), so ExecWait waits for the
      ; real work rather than for a copy in %TEMP%; the uninstaller cannot delete itself
      ; that way, so it and the then-empty folder go here.
      ExecWait '"$R1\uninstall.exe" /UPDATE /S _?=$R1' $R2
      ${If} $R2 = 0
        Delete "$R1\uninstall.exe"
        RMDir "$R1"
        DeleteRegKey HKCU "${uninstRoot}\${oldName}"
        DeleteRegKey HKCU "Software\${MANUFACTURER}\${oldName}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; The version being replaced, before the install section writes this one's (D-279).
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $DzlPrevVersion SHCTX "${UNINSTKEY}" "DisplayVersion"
!macroend

; Applied when everything is installed, which is the only safe moment: an upgrade runs
; the previous version's uninstaller partway through, and anything written before that
; can be deleted by it.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $OptNewsState = 1
    Call DisableNews
  ${EndIf}

  ; The versioned copy of the icon, then every shortcut of ours pointed at it (D-262).
  ; The previous version's copy goes, by name: nothing of ours points at it any more,
  ; and a wildcard here would reach into an `icons` folder the user already had, if
  ; the directory page was pointed at a folder of their own (D-279).
  CreateDirectory "$INSTDIR\icons"
  ${If} $DzlPrevVersion != ""
  ${AndIf} $DzlPrevVersion != "${VERSION}"
    Delete "$INSTDIR\icons\$DzlPrevVersion.ico"
  ${EndIf}
  File "/oname=$INSTDIR\icons\${VERSION}.ico" "${INSTALLERICON}"

  !insertmacro DzlRefreshIconIfOurs "$DESKTOP\${PRODUCTNAME}.lnk"
  !insertmacro DzlRefreshIconIfOurs "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro DzlRefreshIconIfOurs "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"

  ; A pinned taskbar button is Explorer's own copy of a shortcut, kept here under
  ; whatever name it chose; every one that points at this install takes the new icon.
  FindFirst $R8 $R7 "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk"
  ${DoWhile} $R7 != ""
    !insertmacro DzlRefreshIconIfOurs "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\$R7"
    FindNext $R8 $R7
  ${Loop}
  FindClose $R8

  !insertmacro DzlRetireOldProduct "DayZ Launcher" "Software\Microsoft\Windows\CurrentVersion\Uninstall" \
    "$DESKTOP" "$SMPROGRAMS" "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"

  ; Windows' own icon-cache refresh, for the picture it may still hold for the exe's
  ; path itself (a window that falls back to it, a shortcut someone else made). Best
  ; effort, hidden. The 64-bit tool: this installer is 32-bit, where System32 is
  ; redirected to SysWOW64, which has no ie4uinit.exe on this machine (D-262).
  ${If} ${FileExists} "$WINDIR\Sysnative\ie4uinit.exe"
    nsExec::Exec '"$WINDIR\Sysnative\ie4uinit.exe" -show'
    Pop $R6
  ${ElseIf} ${FileExists} "$SYSDIR\ie4uinit.exe"
    nsExec::Exec '"$SYSDIR\ie4uinit.exe" -show'
    Pop $R6
  ${EndIf}
!macroend

; A retired pre-rename copy's shortcuts keep their old name and point at this install,
; so they go with it. Folders are parameters for the same reason as above.
!macro DzlDeleteIfOurs shortcut
  ${If} ${FileExists} "${shortcut}"
    !insertmacro DzlIsOurShortcut "${shortcut}"
    Pop $0
    ${If} $0 = 1
      Delete "${shortcut}"
    ${EndIf}
  ${EndIf}
!macroend

!macro DzlDeleteRetiredShortcuts oldName desktopDir smDir
  !insertmacro DzlDeleteIfOurs "${desktopDir}\${oldName}.lnk"
  !insertmacro DzlDeleteIfOurs "${smDir}\${oldName}.lnk"
  !insertmacro DzlDeleteIfOurs "${smDir}\${oldName}\${oldName}.lnk"
  RMDir "${smDir}\${oldName}"
!macroend

; After the uninstall, not before it (D-279): the template asks about a running launcher
; after the pre-uninstall hook, so a Cancel there kept the install with every shortcut
; pointing at an icon already deleted. Only this version's own copy goes (the wildcard
; could reach a folder of the user's own, D-279), then the two folders, if empty. The
; retired shortcuts go with the install, never on an update's pass through here.
!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\icons\${VERSION}.ico"
  RMDir "$INSTDIR\icons"
  RMDir "$INSTDIR"
  ${If} $UpdateMode <> 1
    !insertmacro DzlDeleteRetiredShortcuts "DayZ Launcher" "$DESKTOP" "$SMPROGRAMS"
  ${EndIf}
!macroend
