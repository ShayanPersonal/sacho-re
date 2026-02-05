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
    
    ; Create manifest file to track installed DLLs for clean uninstallation
    FileOpen $0 "$INSTDIR\installed_dlls.txt" w
    
    ; Enumerate all DLLs in raw_dependencies and copy them
    FindFirst $1 $2 "$INSTDIR\installers\raw_dependencies\*.dll"
    ${IfNot} ${Errors}
        loop_copy:
            ; Copy the DLL to install directory
            CopyFiles /SILENT "$INSTDIR\installers\raw_dependencies\$2" "$INSTDIR"
            ; Write the DLL name to manifest
            FileWrite $0 "$2$\r$\n"
            ; Find next file
            FindNext $1 $2
            ${IfNot} ${Errors}
                Goto loop_copy
            ${EndIf}
        FindClose $1
    ${EndIf}
    
    FileClose $0
    
    DetailPrint "GStreamer runtime libraries installed successfully"
    
    ; Remove the installers folder after copying (no longer needed)
    RMDir /r "$INSTDIR\installers"
!macroend

; ============================================================================
; PREUNINSTALL - Runs before uninstallation starts
; ============================================================================
!macro NSIS_HOOK_PREUNINSTALL
    ; Remove GStreamer DLLs that were copied during installation
    ; Read from manifest file to know exactly which DLLs were installed
    DetailPrint "Removing GStreamer runtime libraries..."
    
    ${If} ${FileExists} "$INSTDIR\installed_dlls.txt"
        FileOpen $0 "$INSTDIR\installed_dlls.txt" r
        ${IfNot} ${Errors}
            loop_delete:
                FileRead $0 $1
                ${IfNot} ${Errors}
                    ; Trim trailing newline/carriage return
                    StrCpy $2 $1 -2
                    ${If} $2 != ""
                        Delete "$INSTDIR\$2"
                        DetailPrint "Removed: $2"
                    ${EndIf}
                    Goto loop_delete
                ${EndIf}
            FileClose $0
        ${EndIf}
        ; Delete the manifest file itself
        Delete "$INSTDIR\installed_dlls.txt"
    ${Else}
        ; Fallback: if manifest doesn't exist, try to delete known DLLs
        ; This handles upgrades from older versions without manifest
        DetailPrint "Manifest not found, using fallback cleanup..."
        Delete "$INSTDIR\gstpbutils-1.0-0.dll"
        Delete "$INSTDIR\gstvideo-1.0-0.dll"
        Delete "$INSTDIR\orc-0.4-0.dll"
        Delete "$INSTDIR\gstaudio-1.0-0.dll"
        Delete "$INSTDIR\gsttag-1.0-0.dll"
        Delete "$INSTDIR\z-1.dll"
        Delete "$INSTDIR\gstbase-1.0-0.dll"
        Delete "$INSTDIR\gstreamer-1.0-0.dll"
        Delete "$INSTDIR\gmodule-2.0-0.dll"
        Delete "$INSTDIR\gobject-2.0-0.dll"
        Delete "$INSTDIR\ffi-7.dll"
        Delete "$INSTDIR\glib-2.0-0.dll"
        Delete "$INSTDIR\pcre2-8-0.dll"
        Delete "$INSTDIR\intl-8.dll"
        Delete "$INSTDIR\gstapp-1.0-0.dll"
        Delete "$INSTDIR\gio-2.0-0.dll"
    ${EndIf}
    
    DetailPrint "GStreamer runtime libraries removed"
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
