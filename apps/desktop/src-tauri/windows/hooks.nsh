; ContextForge NSIS hooks.
; On a real uninstall (not an upgrade), remove ContextForge's user-level agent integrations so Claude Code,
; Codex and Antigravity are not left calling a deleted executable. Only ContextForge's own entries are
; removed; every changed file is backed up first. The memory database is kept.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    nsExec::ExecToLog '"$INSTDIR\contextforge.exe" integrations uninstall all --yes'
    Pop $0
  ${EndIf}
!macroend
