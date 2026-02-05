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
    
    ; The DLLs are bundled in installers/raw_dependencies folder (Tauri preserves relative paths)
    ; Copy all DLLs to the main application directory
    CopyFiles /SILENT "$INSTDIR\installers\raw_dependencies\*.dll" "$INSTDIR"
    
    DetailPrint "GStreamer runtime libraries installed successfully"
    
    ; Remove the installers folder after copying (no longer needed)
    RMDir /r "$INSTDIR\installers"
!macroend

; ============================================================================
; PREUNINSTALL - Runs before uninstallation starts
; ============================================================================
!macro NSIS_HOOK_PREUNINSTALL
    ; Remove GStreamer DLLs that were copied during installation
    DetailPrint "Removing GStreamer runtime libraries..."
    
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
    
    DetailPrint "GStreamer runtime libraries removed"
!macroend

; ============================================================================
; POSTUNINSTALL - Runs after uninstallation completes
; ============================================================================
!macro NSIS_HOOK_POSTUNINSTALL
    ; Clean up any remaining folders
    RMDir /r "$INSTDIR\installers"
    RMDir "$INSTDIR\resources"
!macroend
