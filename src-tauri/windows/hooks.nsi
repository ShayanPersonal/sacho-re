; Sacho NSIS Installer Hooks
; This file contains custom NSIS macros for the Sacho installer
; It handles the private deployment of GStreamer

!include "LogicLib.nsh"
!include "FileFunc.nsh"

; Variables for GStreamer installation
Var GStreamerInstallDir
Var GStreamerMsiPath
Var GStreamerMsiFound

; ============================================================================
; PREINSTALL - Runs before any files are copied
; ============================================================================
!macro NSIS_HOOK_PREINSTALL
    ; Set up GStreamer installation directory inside the app folder
    StrCpy $GStreamerInstallDir "$INSTDIR\gstreamer"
    StrCpy $GStreamerMsiFound "0"
!macroend

; ============================================================================
; POSTINSTALL - Runs after all files are copied but before shortcuts are created
; ============================================================================
!macro NSIS_HOOK_POSTINSTALL
    ; Install GStreamer to the app's private folder
    DetailPrint "Checking for GStreamer runtime..."
    
    ; Look for any GStreamer MSI in the installers folder (bundled with resources)
    ; The file could be named with version numbers, so we search for patterns
    FindFirst $0 $1 "$INSTDIR\installers\gstreamer*.msi"
    ${If} $1 != ""
        StrCpy $GStreamerMsiPath "$INSTDIR\installers\$1"
        StrCpy $GStreamerMsiFound "1"
        DetailPrint "Found GStreamer installer: $1"
    ${EndIf}
    FindClose $0
    
    ; Check if the GStreamer MSI was found
    ${If} $GStreamerMsiFound == "1"
        ; Create the gstreamer directory
        CreateDirectory "$INSTDIR\gstreamer"
        
        ; Run the GStreamer MSI installer silently to the app's folder
        ; /passive shows progress but no user interaction
        ; /qn would be completely silent
        DetailPrint "Installing GStreamer to: $INSTDIR\gstreamer"
        nsExec::ExecToLog 'msiexec /passive INSTALLDIR="$INSTDIR\gstreamer" /i "$GStreamerMsiPath"'
        Pop $0
        
        ${If} $0 != 0
            ; Try with /qn (quiet, no UI) as fallback
            DetailPrint "Retrying with quiet mode..."
            nsExec::ExecToLog 'msiexec /qn INSTALLDIR="$INSTDIR\gstreamer" /i "$GStreamerMsiPath"'
            Pop $0
        ${EndIf}
        
        ${If} $0 == 0
            DetailPrint "GStreamer installed successfully"
        ${Else}
            DetailPrint "Warning: GStreamer installation returned code $0"
            ; Continue anyway - the app will show an error if GStreamer isn't available
        ${EndIf}
        
        ; Delete the MSI after installation to save space
        Delete "$GStreamerMsiPath"
        
        ; Clean up the installers folder if empty
        RMDir "$INSTDIR\installers"
        
        ; Write a marker file so we know GStreamer was installed by us
        FileOpen $0 "$INSTDIR\gstreamer\.sacho-installed" w
        FileWrite $0 "Installed by Sacho installer"
        FileClose $0
    ${Else}
        DetailPrint "GStreamer MSI not found in installer bundle"
        DetailPrint "The application will attempt to use system GStreamer if available"
        DetailPrint "If video features don't work, please install GStreamer from:"
        DetailPrint "https://gstreamer.freedesktop.org/download/"
    ${EndIf}
!macroend

; ============================================================================
; PREUNINSTALL - Runs before uninstallation starts
; ============================================================================
!macro NSIS_HOOK_PREUNINSTALL
    ; Check if we installed GStreamer privately
    ${If} ${FileExists} "$INSTDIR\gstreamer\.sacho-installed"
        DetailPrint "Removing private GStreamer deployment..."
        
        ; Since we deleted the MSI after install, we just remove the folder
        ; This is cleaner than trying to find and run the MSI uninstaller
        RMDir /r "$INSTDIR\gstreamer"
        
        ${If} ${FileExists} "$INSTDIR\gstreamer"
            DetailPrint "Warning: Could not fully remove GStreamer folder"
        ${Else}
            DetailPrint "GStreamer removed successfully"
        ${EndIf}
    ${EndIf}
!macroend

; ============================================================================
; POSTUNINSTALL - Runs after uninstallation completes
; ============================================================================
!macro NSIS_HOOK_POSTUNINSTALL
    ; Clean up the gstreamer folder if it still exists
    ${If} ${FileExists} "$INSTDIR\gstreamer"
        RMDir /r "$INSTDIR\gstreamer"
    ${EndIf}
    
    ; Clean up the resources folder if empty
    RMDir "$INSTDIR\resources"
!macroend
