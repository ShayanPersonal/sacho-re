<script lang="ts">
  // App version (could be fetched from Tauri in the future)
  const appVersion = '1.0.0';
</script>

<div class="about">
  <div class="about-header">
    <h2>About</h2>
  </div>
  
  <div class="about-content">
    <div class="about-card">
      <div class="about-logo">
        <svg viewBox="0 0 64 64" fill="currentColor">
          <circle cx="32" cy="32" r="28" fill="none" stroke="currentColor" stroke-width="2"/>
          <circle cx="32" cy="32" r="8" fill="currentColor"/>
          <path d="M32 12v8M32 44v8M12 32h8M44 32h8" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
        </svg>
      </div>
      
      <div class="about-title">
        <h3>Sacho</h3>
        <span class="version-badge">Version {appVersion}</span>
      </div>
      
      <p class="about-description">
        An unobtrusive recording and organizing tool for live musicians and other spontaneous creatives. Free yourself from the DAW.
      </p>
      
      <div class="about-features">
        <div class="feature">
          <span class="feature-icon">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <rect x="3" y="10" width="3" height="8" rx="1"/>
              <rect x="8" y="6" width="3" height="12" rx="1"/>
              <rect x="13" y="8" width="3" height="10" rx="1"/>
              <rect x="18" y="4" width="3" height="14" rx="1"/>
            </svg>
          </span>
          <span class="feature-label">MIDI Recording</span>
          <span class="feature-desc">Capture every note and expression</span>
        </div>
        <div class="feature">
          <span class="feature-icon">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3z"/>
              <path d="M17 11c0 2.76-2.24 5-5 5s-5-2.24-5-5H5c0 3.53 2.61 6.43 6 6.92V21h2v-3.08c3.39-.49 6-3.39 6-6.92h-2z"/>
            </svg>
          </span>
          <span class="feature-label">Audio Capture</span>
          <span class="feature-desc">High-quality lossless recording</span>
        </div>
        <div class="feature">
          <span class="feature-icon">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <path d="M17 10.5V7c0-.55-.45-1-1-1H4c-.55 0-1 .45-1 1v10c0 .55.45 1 1 1h12c.55 0 1-.45 1-1v-3.5l4 4v-11l-4 4z"/>
            </svg>
          </span>
          <span class="feature-label">Video Recording</span>
          <span class="feature-desc">Capture the moment as it happened</span>
        </div>
      </div>
      
      <div class="about-credits">
        <p class="tech-stack">Built around free and open video formats.</p>
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
    padding-top: 1.25rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
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
</style>
