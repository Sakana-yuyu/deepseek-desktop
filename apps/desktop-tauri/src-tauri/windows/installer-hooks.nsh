!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToStack '"$SYSDIR\taskkill.exe" /F /T /IM "dsh-desktop.exe"'
  Pop $0
  Sleep 750
!macroend

!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$DESKTOP\DeepSeek Harness.lnk" 0 icon_refresh_done
  Delete "$DESKTOP\DeepSeek Harness.lnk"
  CreateShortCut "$DESKTOP\DeepSeek Harness.lnk" "$INSTDIR\dsh-desktop.exe" "" "$INSTDIR\deepseek-harness-icon-0.1.0-rc.5-0.1.ico" 0 SW_SHOWNORMAL

icon_refresh_done:
  ; Explorer caches icons by shortcut and executable path, so notify it after
  ; replacing the shortcut with the versioned standalone icon resource.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
