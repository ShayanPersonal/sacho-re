<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  
  // App version (could be fetched from Tauri in the future)
  const appVersion = '1.0.0';
  
  function openExternal(url: string) {
    openUrl(url);
  }
</script>

<div class="about">
  <div class="about-header">
    <h2>About</h2>
  </div>
  
  <div class="about-content">
    <div class="about-card">
      <div class="about-logo">
        <svg viewBox="0 0 64 64" fill="currentColor">
          <!-- Outer ring -->
          <circle cx="32" cy="32" r="20" fill="none" stroke="currentColor" stroke-width="3"/>
          <!-- Center circle -->
          <circle cx="32" cy="32" r="8" fill="currentColor"/>
          <!-- 8 radiating lines -->
          <path d="M32 4v8M32 52v8" stroke="currentColor" stroke-width="3" stroke-linecap="round"/>
          <path d="M4 32h8M52 32h8" stroke="currentColor" stroke-width="3" stroke-linecap="round"/>
          <path d="M12.2 12.2l5.6 5.6M46.2 46.2l5.6 5.6" stroke="currentColor" stroke-width="3" stroke-linecap="round"/>
          <path d="M51.8 12.2l-5.6 5.6M17.8 46.2l-5.6 5.6" stroke="currentColor" stroke-width="3" stroke-linecap="round"/>
        </svg>
      </div>
      
      <div class="about-title">
        <h3>Sacho</h3>
        <span class="version-badge">Version {appVersion}</span>
      </div>
      
      <p class="about-description">
        The songwriter's dream.
      </p>
      
      <div class="about-features">
        <div class="feature">
          <span class="feature-icon">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <circle cx="12" cy="12" r="3"/>
              <circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" stroke-width="1.5"/>
              <path d="M12 2v3M12 19v3M2 12h3M19 12h3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </span>
          <span class="feature-label">Trigger Recording</span>
          <span class="feature-desc">Automatic MIDI, audio, and video recording when a performance is detected.</span>
        </div>
        <div class="feature">
          <span class="feature-icon">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <circle cx="6" cy="8" r="2.5"/>
              <circle cx="9" cy="5" r="2"/>
              <circle cx="5" cy="12" r="1.5"/>
              <circle cx="17" cy="16" r="2.5"/>
              <circle cx="14" cy="18" r="2"/>
              <circle cx="19" cy="12" r="1.5"/>
              <circle cx="12" cy="12" r="1" opacity="0.5"/>
            </svg>
          </span>
          <span class="feature-label">Similarity Map</span>
          <span class="feature-desc">Quickly locate performances with similar themes, chord progressions, etc.</span>
        </div>
        <div class="feature">
          <span class="feature-icon">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.5"/>
              <path d="M12 7v5l-3 2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
              <path d="M4.5 8A8 8 0 0 1 12 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/>
              <path d="M4 4v4h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
            </svg>
          </span>
          <span class="feature-label">Pre-roll Capture</span>
          <span class="feature-desc">Capture the moments leading up to the performance for a more natural recording.</span>
        </div>
      </div>
      
      <div class="open-source-notice">
        <p class="notice-title">Open Source Licenses</p>
        <p class="notice-text">
          This application uses <button class="link-btn" onclick={() => openExternal('https://gstreamer.freedesktop.org/')}>GStreamer</button>, 
          a multimedia framework licensed under the 
          <button class="link-btn" onclick={() => openExternal('https://www.gnu.org/licenses/lgpl-2.1.html')}>GNU Lesser General Public License (LGPL)</button>. 
          The complete source code for this version of GStreamer is available at 
          <button class="link-btn" onclick={() => openExternal('https://gitlab.freedesktop.org/gstreamer/gstreamer/-/tree/1.26.10')}>gitlab.freedesktop.org/gstreamer/-/tree/1.26.10</button>. 
          The GStreamer dynamic libraries used by this application are located in the application installation folder.
        </p>
      </div>
      
      <div class="disclaimer">
        <p>
          This software is provided "as-is" without warranty of any kind, express or implied. 
          In no event shall the author be liable for any claim, damages, or other liability 
          arising from the use of this software. Use at your own risk.
        </p>
      </div>
    </div>
  </div>
</div>

<style>
  .about {
    display: flex;
    flex-direction: column;
    height: 100%;
    gap: 1.5rem;
    min-height: 0;
  }
  
  .about-header {
    flex-shrink: 0;
  }
  
  .about-header h2 {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
    font-size: 1.375rem;
    font-weight: 400;
    color: #e8e6e3;
    letter-spacing: 0.06em;
  }
  
  .about-content {
    flex: 1;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 2rem;
    padding-bottom: 2rem;
    overflow-y: auto;
    min-height: 0;
  }
  
  .about-card {
    background: rgba(255, 255, 255, 0.015);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    padding: 2.5rem 3rem;
    max-width: 480px;
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
  
  .about-title {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
  }
  
  .about-title h3 {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
    font-size: 2rem;
    font-weight: 400;
    color: #e8e6e3;
    letter-spacing: 0.1em;
    margin: 0;
  }
  
  .version-badge {
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
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
  
  .about-credits {
    margin-top: 0.75rem;
    width: 100%;
  }
  
  .about-credits p {
    margin: 0;
  }
  
  .tech-stack {
    font-size: 0.75rem;
    color: #5a5a5a;
  }
  
  .tagline {
    margin-top: 0.5rem !important;
    font-size: 0.75rem;
    font-style: italic;
    color: #4a4a4a;
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
  :global(body.light-mode) .about-header h2 {
    color: #2a2a2a;
  }

  :global(body.light-mode) .about-card {
    background: rgba(255, 255, 255, 0.85);
    border-color: rgba(0, 0, 0, 0.08);
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

  :global(body.light-mode) .tech-stack {
    color: #6a6a6a;
  }

  :global(body.light-mode) .tagline {
    color: #7a7a7a;
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

  :global(body.light-mode) .disclaimer {
    border-top-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .disclaimer p {
    color: #7a7a7a;
  }
</style>
