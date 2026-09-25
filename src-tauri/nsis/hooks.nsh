; Installer hooks (D-206). Tauri's supported extension point — `installerHooks` in
; tauri.conf.json — so nothing here counts as a deviation from the stock template and
; `nsis_template_check.js` has nothing to say about it.
;
; This file is included before the template defines VERSION, PRODUCTNAME and the rest,
; so everything that uses them is a macro: macros expand where they are inserted, by
; which point the defines exist.

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

; Pushes 1 when `shortcut` points at this install's exe, else 0. The path is read
; expanded (flags 0, where Tauri's IsShortcutTarget reads it raw), so a link stored with
; an environment variable in it still matches; the comparison ignores case. Uses $0–$3.
!macro DzlIsOurShortcut shortcut
  StrCpy $3 0
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}", .r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READ})'
      System::Alloc 520
      Pop $2
      ${IShellLink::GetPath} $0 '(.r2, 260, 0, 0)'
      ${If} $2 == "$INSTDIR\${MAINBINARYNAME}.exe"
        StrCpy $3 1
      ${EndIf}
      System::Free $2
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  Push $3
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

; Applied when everything is installed, which is the only safe moment: an upgrade runs
; the previous version's uninstaller partway through, and anything written before that
; can be deleted by it.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $OptNewsState = 1
    Call DisableNews
  ${EndIf}

  ; The versioned copy of the icon, then every shortcut of ours pointed at it (D-262).
  ; The previous version's copy goes: nothing of ours points at it any more.
  CreateDirectory "$INSTDIR\icons"
  Delete "$INSTDIR\icons\*.ico"
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

; The uninstaller removes the icon copy before the template's `RMDir "$INSTDIR"`, which
; only removes an empty folder (D-262).
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\icons\*.ico"
  RMDir "$INSTDIR\icons"
!macroend
