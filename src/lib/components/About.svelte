<script lang="ts">
    import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
    import { getVersion } from "@tauri-apps/api/app";
    import { appConfigDir, appDataDir, join } from "@tauri-apps/api/path";
    import { onMount } from "svelte";
    import { resetCache, resetSettings } from "$lib/api";
    import { importedFiles, selectedFileId, similarFiles } from "$lib/stores/similarity";

    let { open = false, onclose }: { open: boolean; onclose: () => void } =
        $props();

    let appVersion = $state("...");
    let confirmAction = $state<"cache" | "settings" | null>(null);
    let isResetting = $state(false);

    onMount(async () => {
        appVersion = await getVersion();
    });

    function openExternal(url: string) {
        openUrl(url);
    }

    function handleOverlayClick(e: MouseEvent) {
        if (e.target === e.currentTarget) {
            if (confirmAction) {
                confirmAction = null;
            } else {
                onclose();
            }
        }
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === "Escape") {
            if (confirmAction) {
                confirmAction = null;
            } else {
                onclose();
            }
        }
    }

    async function viewFiles() {
        try {
            if (confirmAction === "cache") {
                const dir = await appDataDir();
                const dbPath = await join(dir, "sessions.db");
                await revealItemInDir(dbPath);
            } else if (confirmAction === "settings") {
                const dir = await appConfigDir();
                const configPath = await join(dir, "config.toml");
                await revealItemInDir(configPath);
            }
        } catch (e) {
            console.error("Failed to open directory:", e);
        }
    }

    async function handleConfirm() {
        if (!confirmAction) return;
        const action = confirmAction;
        isResetting = true;
        try {
            if (action === "cache") {
                await resetCache();
                importedFiles.set([]);
                selectedFileId.set(null);
                similarFiles.set([]);
            } else {
                await resetSettings();
                window.location.reload();
            }
        } catch (e) {
            console.error("Reset failed:", e);
        } finally {
            isResetting = false;
            confirmAction = null;
        }
    }
</script>

{#if open}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
        class="about-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="About Sacho"
        tabindex="-1"
        onclick={handleOverlayClick}
        onkeydown={handleKeydown}
    >
        <div class="about-modal">
            <button
                class="about-close"
                onclick={onclose}
                title="Close"
            >&times;</button>

            <div class="about-card">
                <div class="about-logo">
                    <svg viewBox="0 0 64 64" fill="currentColor">
                        <!-- Outer ring -->
                        <circle class="ring" cx="32" cy="32" r="20" fill="none" stroke="currentColor" stroke-width="3" />
                        <!-- Center circle -->
                        <circle cx="32" cy="32" r="8" fill="currentColor" />
                        <!-- 8 radiating rays (clockwise from top, individually animated) -->
                        <path class="ray ray-0" d="M32 4v8" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-1" d="M51.8 12.2l-5.6 5.6" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-2" d="M52 32h8" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-3" d="M46.2 46.2l5.6 5.6" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-4" d="M32 52v8" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-5" d="M17.8 46.2l-5.6 5.6" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-6" d="M4 32h8" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                        <path class="ray ray-7" d="M12.2 12.2l5.6 5.6" stroke="currentColor" stroke-width="3" stroke-linecap="round" fill="none" />
                    </svg>
                </div>

                <div class="about-title">
                    <h3>Sacho<sup class="tm">&trade;</sup></h3>
                </div>

                <div class="about-description">
                    <p>The Songwriter's Notebook&trade;</p>
                    <span class="version-badge">Version {appVersion}</span>
                </div>

                <div class="about-features">
                    <div class="feature">
                        <span class="feature-icon icon-red">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                                <circle cx="12" cy="12" r="10" />
                                <circle cx="12" cy="12" r="3" fill="currentColor" />
                            </svg>
                        </span>
                        <span class="feature-label">Auto-Capture</span>
                        <span class="feature-desc"
                            >Automatic MIDI, audio, and video recording when playing
                            is detected.</span
                        >
                    </div>
                    <div class="feature">
                        <span class="feature-icon icon-blue">
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.5"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <!-- fragmented pre-roll -->
                                <path d="M2 13l0.5-2" opacity="0.35" />
                                <path d="M4 11.5l0.6 1" opacity="0.45" />
                                <path d="M6 12l0.7-1.5 0.7 3 0.7-1.5" opacity="0.55" />
                                <path d="M9.5 12l0.7-2.5 0.8 5 0.7-2.5" opacity="0.7" />
                                <!-- solid waveform -->
                                <path d="M12 12l1.5-5 2 10 2-10 1.5 5h3" />
                            </svg>
                        </span>
                        <span class="feature-label">Pre-roll Capture</span>
                        <span class="feature-desc"
                            >Capture the moments leading into the performance - don't miss a beat.</span
                        >
                    </div>
                    <div class="feature">
                        <span class="feature-icon icon-green">
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.5"
                            >
                                <circle cx="8.5" cy="12" r="7" />
                                <circle cx="15.5" cy="12" r="7" />
                                <path d="M12 5.96A7 7 0 0 1 12 18.04A7 7 0 0 1 12 5.96Z" fill="currentColor" opacity="0.25" stroke="none" />
                            </svg>
                        </span>
                        <span class="feature-label">Find Similar</span>
                        <span class="feature-desc"
                            >Quickly locate files with similar melodies and
                            harmonies - put your old recordings to use.</span
                        >
                    </div>
                </div>

                <div class="open-source-notice">
                    <p class="notice-title">Open Source Licenses</p>
                    <p class="notice-text">
                        This application uses <button
                            class="link-btn"
                            onclick={() =>
                                openExternal("https://gstreamer.freedesktop.org/")}
                            >GStreamer</button
                        >, a multimedia framework licensed under the
                        <button
                            class="link-btn"
                            onclick={() =>
                                openExternal(
                                    "https://www.gnu.org/licenses/lgpl-2.1.html",
                                )}
                            >GNU Lesser General Public License (LGPL) v2.1</button
                        >. The complete source code for the version of GStreamer used is
                        available at
                        <button
                            class="link-btn"
                            onclick={() =>
                                openExternal(
                                    "https://gitlab.freedesktop.org/gstreamer/gstreamer/-/tree/1.26.10",
                                )}
                            >gitlab.freedesktop.org/gstreamer/-/tree/1.26.10</button
                        >.
                    </p>
                    <p class="notice-text">
                        This software uses libraries from the <button
                            class="link-btn"
                            onclick={() => openExternal("https://ffmpeg.org/")}
                            >FFmpeg</button
                        >
                        project under the
                        <button
                            class="link-btn"
                            onclick={() =>
                                openExternal(
                                    "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html",
                                )}>LGPLv2.1</button
                        >. The complete source code for the version of FFmpeg used is
                        available at
                        <button
                            class="link-btn"
                            onclick={() =>
                                openExternal(
                                    "https://www.ffmpeg.org/releases/ffmpeg-7.1.1.tar.gz",
                                )}>ffmpeg.org/releases/ffmpeg-7.1.1.tar.gz</button
                        >. It was compiled with:
                        <code class="notice-code"
                            >--toolchain=msvc --enable-shared --disable-static
                            --disable-programs --disable-doc --disable-everything
                            --enable-encoder=ffv1 --enable-decoder=ffv1
                            --disable-avdevice --disable-postproc --disable-network
                            --disable-autodetect</code
                        >. FFmpeg is a trademark of Fabrice Bellard, originator of
                        the FFmpeg project.
                    </p>
                </div>

                <div class="disclaimer">
                    <p>
                        This software is provided "as-is" without warranty of any
                        kind, express or implied. In no event shall the author be
                        liable for any claim, damages, or other liability arising
                        from the use of this software. Use at your own risk.
                    </p>
                </div>

                <div class="reset-section">
                    <div class="reset-buttons">
                        <button
                            class="reset-btn"
                            onclick={() => confirmAction = "cache"}
                            disabled={isResetting}
                        >Reset Cache</button>
                        <button
                            class="reset-btn"
                            onclick={() => confirmAction = "settings"}
                            disabled={isResetting}
                        >Reset Settings</button>
                    </div>
                </div>
            </div>

        </div>

        {#if confirmAction}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div
                class="confirm-overlay"
                role="dialog"
                aria-modal="true"
                aria-label="Confirm reset"
                tabindex="-1"
                onclick={(e) => { if (e.target === e.currentTarget) confirmAction = null; }}
                onkeydown={(e) => { if (e.key === "Escape") confirmAction = null; }}
            >
                <div class="confirm-dialog">
                    {#if confirmAction === "cache"}
                        <p class="confirm-title">Reset Cache</p>
                        <p class="confirm-text">The cache is used for UI, search, and similarity. The application will rescan your recordings folder. This may take some time.</p>
                    {:else}
                        <p class="confirm-title">Reset Settings</p>
                        <p class="confirm-text">This will restore all settings to their defaults. Device configurations will be reset. The app will reload.</p>
                    {/if}
                    <div class="confirm-actions">
                        <button
                            class="confirm-view"
                            onclick={viewFiles}
                            disabled={isResetting}
                        >View file</button>
                        <div class="confirm-actions-right">
                            <button
                                class="confirm-cancel"
                                onclick={() => confirmAction = null}
                                disabled={isResetting}
                            >Cancel</button>
                            <button
                                class="confirm-proceed"
                                onclick={handleConfirm}
                                disabled={isResetting}
                            >{isResetting ? "Resetting..." : "Reset"}</button>
                        </div>
                    </div>
                </div>
            </div>
        {/if}
    </div>
{/if}

<style>
    .about-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.6);
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 1000;
    }

    .about-modal {
        position: relative;
        max-width: 520px;
        max-height: 85vh;
        overflow-y: auto;
        border-radius: 0.5rem;
        background: #1a1917;
        border: 1px solid rgba(255, 255, 255, 0.08);
        box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
        padding: 2rem;
    }

    .about-close {
        position: absolute;
        top: 0.75rem;
        right: 0.75rem;
        background: none;
        border: none;
        color: #8a8a8a;
        font-size: 1.5rem;
        line-height: 1;
        cursor: pointer;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        transition: all 0.15s ease;
    }

    .about-close:hover {
        color: #e8e6e3;
        background: rgba(255, 255, 255, 0.08);
    }

    .about-card {
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 1.25rem;
    }

    .about-logo {
        width: 72px;
        height: 72px;
        color: #c9a962;
        opacity: 0.9;
    }

    .about-logo svg {
        width: 100%;
        height: 100%;
    }

    /* Ring expand animation (100ms initial delay) */
    .ring {
        transform-box: fill-box;
        transform-origin: center;
        animation: ring-expand 0.7s cubic-bezier(0.22, 1, 0.36, 1) 0.1s both;
    }

    @keyframes ring-expand {
        0% { transform: scale(0.82); opacity: 0.4; }
        100% { transform: scale(1); opacity: 1; }
    }

    /* Ray wave — starts as ring expand is finishing */
    .ray-0 { animation: ray-pump-0 0.42s cubic-bezier(0.33, 1, 0.68, 1) 0.3s, ray-pump-0-end 0.42s cubic-bezier(0.33, 1, 0.68, 1) 1.04s; }
    .ray-1 { animation: ray-pump-1 0.38s cubic-bezier(0.33, 1, 0.68, 1) 0.4s; }
    .ray-2 { animation: ray-pump-2 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.5s; }
    .ray-3 { animation: ray-pump-3 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.58s; }
    .ray-4 { animation: ray-pump-4 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.66s; }
    .ray-5 { animation: ray-pump-5 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.74s; }
    .ray-6 { animation: ray-pump-6 0.42s cubic-bezier(0.33, 1, 0.68, 1) 0.84s; }
    .ray-7 { animation: ray-pump-7 0.38s cubic-bezier(0.33, 1, 0.68, 1) 0.94s; }

    @keyframes ray-pump-0 { /* top */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(0, -5px); }
    }
    @keyframes ray-pump-0-end { /* top — settling */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(0, -2px); }
    }
    @keyframes ray-pump-1 { /* top-right */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(3.5px, -3.5px); }
    }
    @keyframes ray-pump-2 { /* right */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(5px, 0); }
    }
    @keyframes ray-pump-3 { /* bottom-right */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(3.5px, 3.5px); }
    }
    @keyframes ray-pump-4 { /* bottom */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(0, 5px); }
    }
    @keyframes ray-pump-5 { /* bottom-left */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(-3.5px, 3.5px); }
    }
    @keyframes ray-pump-6 { /* left */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(-5px, 0); }
    }
    @keyframes ray-pump-7 { /* top-left */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(-2.5px, -2.5px); }
    }

    .about-title {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.5rem;
    }

    .about-title h3 {
        font-family: "Roboto", -apple-system, BlinkMacSystemFont, sans-serif;
        font-size: 1.5rem;
        font-weight: 500;
        color: #e8e6e3;
        letter-spacing: 0.1em;
        margin: 0;
    }

    .tm {
        font-size: 0.4em;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        font-weight: 300;
        opacity: 0.7;
        vertical-align: super;
        margin-left: 0.05em;
    }

    .version-badge {
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.625rem;
        color: #3a3a3a;
        margin-top: 0.5rem;
    }

    .about-description {
        font-size: 0.875rem;
        color: #6b6b6b;
        line-height: 1.7;
        max-width: 360px;
        margin-top: -0.75rem;
        white-space: pre-line;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.125rem;
    }

    .about-features {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        width: 100%;
        margin-top: 0.5rem;
        padding-top: 1.25rem;
        border-top: 1px solid rgba(255, 255, 255, 0.04);
    }

    .feature {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        text-align: left;
        padding: 0.75rem;
        background: rgba(255, 255, 255, 0.02);
        border-radius: 0.25rem;
    }

    .feature-icon {
        width: 28px;
        height: 28px;
        flex-shrink: 0;
    }

    .icon-red { color: #7a5555; }
    .icon-blue { color: #55697a; }
    .icon-green { color: #557a5e; }

    .feature-icon svg {
        width: 100%;
        height: 100%;
    }

    .feature-label {
        font-size: 0.8125rem;
        font-weight: 500;
        color: #a8a8a8;
        min-width: 110px;
    }

    .feature-desc {
        font-size: 0.75rem;
        color: #5a5a5a;
    }

    .open-source-notice {
        margin-top: 0.75rem;
        padding-top: 1.25rem;
        border-top: 1px solid rgba(255, 255, 255, 0.04);
        width: 100%;
    }

    .notice-title {
        font-size: 0.6875rem;
        font-weight: 500;
        color: #6a6a6a;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        margin: 0 0 0.5rem 0;
        text-align: center;
    }

    .notice-text {
        font-size: 0.625rem;
        color: #5a5a5a;
        line-height: 1.7;
        margin: 0;
        text-align: center;
    }

    .notice-text + .notice-text {
        margin-top: 0.75rem;
    }

    .notice-text .link-btn {
        background: none;
        border: none;
        padding: 0;
        font: inherit;
        color: #c9a962;
        text-decoration: none;
        cursor: pointer;
    }

    .notice-text .link-btn:hover {
        text-decoration: underline;
    }

    .notice-code {
        font-family: monospace;
        font-size: 0.5625rem;
        color: #4a4a4a;
        word-break: break-all;
    }

    .disclaimer {
        margin-top: 0.75rem;
        padding-top: 1.25rem;
        border-top: 1px solid rgba(255, 255, 255, 0.04);
        width: 100%;
    }

    .disclaimer p {
        font-size: 0.625rem;
        color: #4a4a4a;
        line-height: 1.6;
        margin: 0;
        text-align: center;
    }

    /* Light mode overrides */
    :global(body.light-mode) .about-overlay {
        background: rgba(0, 0, 0, 0.35);
    }

    :global(body.light-mode) .about-modal {
        background: #ffffff;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 24px 64px rgba(0, 0, 0, 0.2);
    }

    :global(body.light-mode) .about-close {
        color: #8a8a8a;
    }

    :global(body.light-mode) .about-close:hover {
        color: #2a2a2a;
        background: rgba(0, 0, 0, 0.06);
    }

    :global(body.light-mode) .about-card {
        background: transparent;
    }

    :global(body.light-mode) .about-logo {
        color: #a08030;
    }

    :global(body.light-mode) .about-title h3 {
        color: #2a2a2a;
    }

    :global(body.light-mode) .version-badge {
        color: #aaaaaa;
    }

    :global(body.light-mode) .about-description {
        color: #5a5a5a;
    }

    :global(body.light-mode) .about-features {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .feature {
        background: rgba(0, 0, 0, 0.03);
    }

    :global(body.light-mode) .icon-red { color: #9a7a7a; }
    :global(body.light-mode) .icon-blue { color: #7a8a9a; }
    :global(body.light-mode) .icon-green { color: #7a9a80; }

    :global(body.light-mode) .feature-label {
        color: #3a3a3a;
    }

    :global(body.light-mode) .feature-desc {
        color: #6a6a6a;
    }

    :global(body.light-mode) .open-source-notice {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .notice-title {
        color: #5a5a5a;
    }

    :global(body.light-mode) .notice-text {
        color: #6a6a6a;
    }

    :global(body.light-mode) .notice-text .link-btn {
        color: #8a6a20;
    }

    :global(body.light-mode) .notice-code {
        color: #7a7a7a;
    }

    :global(body.light-mode) .disclaimer {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .disclaimer p {
        color: #7a7a7a;
    }

    /* Reset section */
    .reset-section {
        margin-top: 0.75rem;
        padding-top: 1.25rem;
        border-top: 1px solid rgba(255, 255, 255, 0.04);
        width: 100%;
    }

    .reset-buttons {
        display: flex;
        gap: 0.75rem;
        justify-content: center;
    }

    .reset-btn {
        font-size: 0.6875rem;
        color: #6a6a6a;
        background: rgba(255, 255, 255, 0.03);
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
        padding: 0.4rem 1rem;
        cursor: pointer;
        transition: all 0.15s ease;
    }

    .reset-btn:hover:not(:disabled) {
        color: #a8a8a8;
        background: rgba(255, 255, 255, 0.06);
        border-color: rgba(255, 255, 255, 0.1);
    }

    .reset-btn:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    /* Confirmation dialog */
    .confirm-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.5);
        backdrop-filter: blur(2px);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 1001;
    }

    .confirm-dialog {
        background: #222120;
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: 0.375rem;
        padding: 1.25rem 1.5rem;
        max-width: 340px;
        box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    }

    .confirm-title {
        font-size: 0.8125rem;
        font-weight: 500;
        color: #e8e6e3;
        margin: 0 0 0.5rem 0;
    }

    .confirm-text {
        font-size: 0.6875rem;
        color: #8a8a8a;
        line-height: 1.6;
        margin: 0 0 1rem 0;
    }

    .confirm-actions {
        display: flex;
        gap: 0.5rem;
        justify-content: space-between;
        align-items: center;
    }

    .confirm-actions-right {
        display: flex;
        gap: 0.5rem;
    }

    .confirm-view {
        font-size: 0.6875rem;
        color: #6a6a6a;
        background: none;
        border: none;
        padding: 0.35rem 0;
        cursor: pointer;
        transition: color 0.15s ease;
    }

    .confirm-view:hover:not(:disabled) {
        color: #a8a8a8;
    }

    .confirm-cancel {
        font-size: 0.6875rem;
        color: #8a8a8a;
        background: none;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 0.25rem;
        padding: 0.35rem 0.875rem;
        cursor: pointer;
        transition: all 0.15s ease;
    }

    .confirm-cancel:hover:not(:disabled) {
        color: #c8c8c8;
        border-color: rgba(255, 255, 255, 0.15);
    }

    .confirm-proceed {
        font-size: 0.6875rem;
        color: #e8e6e3;
        background: #8b3a3a;
        border: 1px solid #a04444;
        border-radius: 0.25rem;
        padding: 0.35rem 0.875rem;
        cursor: pointer;
        transition: all 0.15s ease;
    }

    .confirm-proceed:hover:not(:disabled) {
        background: #a04444;
    }

    .confirm-proceed:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    /* Light mode overrides for reset section */
    :global(body.light-mode) .reset-section {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .reset-btn {
        color: #6a6a6a;
        background: rgba(0, 0, 0, 0.03);
        border-color: rgba(0, 0, 0, 0.1);
    }

    :global(body.light-mode) .reset-btn:hover:not(:disabled) {
        color: #3a3a3a;
        background: rgba(0, 0, 0, 0.06);
        border-color: rgba(0, 0, 0, 0.15);
    }

    :global(body.light-mode) .confirm-overlay {
        background: rgba(0, 0, 0, 0.3);
    }

    :global(body.light-mode) .confirm-dialog {
        background: #ffffff;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 8px 32px rgba(0, 0, 0, 0.15);
    }

    :global(body.light-mode) .confirm-title {
        color: #2a2a2a;
    }

    :global(body.light-mode) .confirm-text {
        color: #5a5a5a;
    }

    :global(body.light-mode) .confirm-view {
        color: #7a7a7a;
    }

    :global(body.light-mode) .confirm-view:hover:not(:disabled) {
        color: #3a3a3a;
    }

    :global(body.light-mode) .confirm-cancel {
        color: #5a5a5a;
        border-color: rgba(0, 0, 0, 0.12);
    }

    :global(body.light-mode) .confirm-cancel:hover:not(:disabled) {
        color: #2a2a2a;
        border-color: rgba(0, 0, 0, 0.2);
    }

    :global(body.light-mode) .confirm-proceed {
        color: #fff;
        background: #c53030;
        border-color: #c53030;
    }

    :global(body.light-mode) .confirm-proceed:hover:not(:disabled) {
        background: #b52828;
    }
</style>
