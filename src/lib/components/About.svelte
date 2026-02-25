<script lang="ts">
    import { openUrl } from "@tauri-apps/plugin-opener";
    import { getVersion } from "@tauri-apps/api/app";
    import { onMount } from "svelte";

    let { open = false, onclose }: { open: boolean; onclose: () => void } =
        $props();

    let appVersion = $state("...");

    onMount(async () => {
        appVersion = await getVersion();
    });

    function openExternal(url: string) {
        openUrl(url);
    }

    function handleOverlayClick(e: MouseEvent) {
        if (e.target === e.currentTarget) {
            onclose();
        }
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === "Escape") {
            onclose();
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
                    <span class="version-badge">Version {appVersion}</span>
                </div>

                <div class="about-description">
                    <p>The Songwriter's Notebook&trade;</p>
                </div>

                <div class="about-features">
                    <div class="feature">
                        <span class="feature-icon">
                            <svg viewBox="0 0 24 24" fill="currentColor">
                                <circle cx="12" cy="12" r="3" />
                                <circle
                                    cx="12"
                                    cy="12"
                                    r="9"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="1.5"
                                />
                            </svg>
                        </span>
                        <span class="feature-label">Auto-Capture</span>
                        <span class="feature-desc"
                            >Automatic MIDI, audio, and video recording when playing
                            is detected.</span
                        >
                    </div>
                    <div class="feature">
                        <span class="feature-icon">
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
                            >Capture the moments leading up to the performance - don't miss a beat.</span
                        >
                    </div>
                    <div class="feature">
                        <span class="feature-icon">
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
                        <span class="feature-label">Similar MIDI</span>
                        <span class="feature-desc"
                            >Quickly locate captures with similar themes and
                            chords - put your past sessions to use.</span
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
            </div>
        </div>
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

    /* Ring expand animation */
    .ring {
        transform-box: fill-box;
        transform-origin: center;
        animation: ring-expand 0.7s cubic-bezier(0.22, 1, 0.36, 1);
    }

    @keyframes ring-expand {
        0% { transform: scale(0.82); opacity: 0.4; }
        100% { transform: scale(1); opacity: 1; }
    }

    /* Ray wave — slow start, fast middle, slow finish */
    .ray-0 { animation: ray-pump-0 0.42s cubic-bezier(0.33, 1, 0.68, 1) 0.05s, ray-pump-0-end 0.42s cubic-bezier(0.33, 1, 0.68, 1) 0.79s; }
    .ray-1 { animation: ray-pump-1 0.38s cubic-bezier(0.33, 1, 0.68, 1) 0.15s; }
    .ray-2 { animation: ray-pump-2 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.25s; }
    .ray-3 { animation: ray-pump-3 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.33s; }
    .ray-4 { animation: ray-pump-4 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.41s; }
    .ray-5 { animation: ray-pump-5 0.35s cubic-bezier(0.33, 1, 0.68, 1) 0.49s; }
    .ray-6 { animation: ray-pump-6 0.42s cubic-bezier(0.33, 1, 0.68, 1) 0.59s; }
    .ray-7 { animation: ray-pump-7 0.38s cubic-bezier(0.33, 1, 0.68, 1) 0.69s; }

    @keyframes ray-pump-0 { /* top */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(0, -5px); }
    }
    @keyframes ray-pump-0-end { /* top — settling */
        0%, 100% { transform: translate(0, 0); }
        50% { transform: translate(0, -3px); }
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
        50% { transform: translate(-3.5px, -3.5px); }
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
        font-size: 0.6875rem;
        color: #5a5a5a;
        padding: 0.25rem 0.75rem;
        background: rgba(255, 255, 255, 0.03);
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
    }

    .about-description {
        font-size: 0.875rem;
        color: #6b6b6b;
        line-height: 1.7;
        max-width: 360px;
        margin: 0;
        white-space: pre-line;
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
        color: #8a8a8a;
        flex-shrink: 0;
    }

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
        color: #6a6a6a;
        background: rgba(0, 0, 0, 0.04);
        border-color: rgba(0, 0, 0, 0.1);
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

    :global(body.light-mode) .feature-icon {
        color: #5a5a5a;
    }

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
</style>
