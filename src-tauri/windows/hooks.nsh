!macro NSIS_HOOK_PREINSTALL
  ; Runs directly before Tauri's CheckIfAppIsRunning macro (verified in
  ; tauri-bundler installer.nsi). Terminate any lingering Nexora or Whisper
  ; processes cleanly so NSIS never aborts with "Failed to kill Nexora".
  nsExec::ExecToLog 'taskkill /F /IM whisper-server.exe /T'
  nsExec::ExecToLog 'taskkill /F /IM whisper-cli.exe /T'
  nsExec::ExecToLog 'taskkill /F /IM nexora.exe /T'
  Sleep 500
!macroend
