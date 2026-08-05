; Tauri removes the app's roaming/local directories when the user selects
; "Delete application data". BJUT-AL also stores its encrypted configuration
; as a Windows Generic Credential, which is outside those directories.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    DetailPrint "Removing BJUT-AL secure configuration"
    nsExec::ExecToLog '"$SYSDIR\cmdkey.exe" /delete:"app-config.cn.edu.bjut.al"'
    Pop $0
  ${EndIf}
!macroend
