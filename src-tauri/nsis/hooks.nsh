; Installer hooks (D-206). Tauri's supported extension point — `installerHooks` in
; tauri.conf.json — so nothing here counts as a deviation from the stock template and
; `nsis_template_check.js` has nothing to say about it.

; Applied when everything is installed, which is the only safe moment: an upgrade runs
; the previous version's uninstaller partway through, and anything written before that
; can be deleted by it.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $OptNewsState = 1
    Call DisableNews
  ${EndIf}
!macroend
