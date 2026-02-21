; Sacho NSIS Installer Hooks
; This file contains custom NSIS macros for the Sacho installer
; It copies pre-bundled GStreamer DLLs to the application directory

; ============================================================================
; PREINSTALL - Runs before any files are copied
; ============================================================================
!macro NSIS_HOOK_PREINSTALL
    ; Nothing needed here
!macroend

; ============================================================================
; POSTINSTALL - Runs after all files are copied but before shortcuts are created
; ============================================================================
!macro NSIS_HOOK_POSTINSTALL
    ; Copy GStreamer DLLs from the bundled resources to the application directory
    ; These DLLs must be in the same directory as the exe for Windows to find them at startup
    DetailPrint "Copying GStreamer runtime libraries..."

    FindFirst $0 $1 "$INSTDIR\installers\dlls\*.dll"
    ${IfNot} ${Errors}
        loop_copy:
            CopyFiles /SILENT "$INSTDIR\installers\dlls\$1" "$INSTDIR"
            FindNext $0 $1
            ${IfNot} ${Errors}
                Goto loop_copy
            ${EndIf}
        FindClose $0
    ${EndIf}

    DetailPrint "GStreamer runtime libraries installed successfully"
    
    ; Remove the installers folder after copying (no longer needed)
    RMDir /r "$INSTDIR\installers"
    
    ; ---- Autostart registration ----
    ; Write to HKLM for all-users installs, HKCU for per-user installs.
    ; Never write both -- Windows processes both Run keys on boot, which
    ; would launch two instances and the single-instance handler would
    ; immediately show the hidden window.
    !if "${INSTALLMODE}" == "both"
    ${If} $MultiUser.InstallMode == "AllUsers"
        WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --autostarted'
        DetailPrint "Registered autostart for all users (HKLM)"
    ${Else}
        WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --autostarted'
        DetailPrint "Registered autostart for current user (HKCU)"
    ${EndIf}
    !endif
    !if "${INSTALLMODE}" == "perMachine"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --autostarted'
    DetailPrint "Registered autostart for all users (HKLM)"
    !endif
    !if "${INSTALLMODE}" == "currentUser"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --autostarted'
    DetailPrint "Registered autostart for current user (HKCU)"
    !endif
!macroend

; ============================================================================
; PREUNINSTALL - Runs before uninstallation starts
; ============================================================================
!macro NSIS_HOOK_PREUNINSTALL
    ; Remove GStreamer DLLs that were copied to $INSTDIR during installation.
    ; Enumerate and delete all *.dll — the only DLLs in $INSTDIR are ours.
    DetailPrint "Removing GStreamer runtime libraries..."

    FindFirst $0 $1 "$INSTDIR\*.dll"
    ${IfNot} ${Errors}
        loop_delete:
            Delete "$INSTDIR\$1"
            DetailPrint "Removed: $1"
            FindNext $0 $1
            ${IfNot} ${Errors}
                Goto loop_delete
            ${EndIf}
        FindClose $0
    ${EndIf}

    ; Clean up manifest from older installs
    Delete "$INSTDIR\installed_dlls.txt"

    DetailPrint "GStreamer runtime libraries removed"

    ; ---- Autostart cleanup ----
    ; Clean up both HKCU and HKLM autostart entries.
    ; Old installs may have written both; remove either to prevent double-launch.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
    DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
    DetailPrint "Removed autostart registry entries"
!macroend

; ============================================================================
; POSTUNINSTALL - Runs after uninstallation completes
; ============================================================================
!macro NSIS_HOOK_POSTUNINSTALL
    ; Clean up any remaining folders
    RMDir /r "$INSTDIR\installers"
    RMDir "$INSTDIR\resources"
    
    ; Try to remove the install directory (will only succeed if empty)
    ; This respects user files - if they added anything, the folder stays
    RMDir "$INSTDIR"
!macroend
