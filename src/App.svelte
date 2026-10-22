<script lang="ts">
  import { onMount } from 'svelte';
  let status = 'No .cmap package installed';
  let fullscreen = false;
  onMount(async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      fullscreen = await getCurrentWindow().isFullscreen();
    } catch { /* browser preview */ }
  });
  async function toggleFullscreen() {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const window = getCurrentWindow();
      await window.setFullscreen(!await window.isFullscreen());
      fullscreen = await window.isFullscreen();
    } catch { status = 'Fullscreen control requires the desktop application.'; }
  }
</script>

<svelte:head><title>CindrixOps CMAP Viewer</title></svelte:head>
<div class="app">
  <header><div class="brand"><span class="mark">C</span><div><h1>CindrixOps <strong>CMAP Viewer</strong></h1><small>OFFLINE MAP VIEWER · WINDOWS FOUNDATION</small></div></div><button onclick={toggleFullscreen}>{fullscreen ? 'Exit fullscreen' : 'Fullscreen'}</button></header>
  <main><div class="empty"><div class="symbol">⌖</div><h2>Map library</h2><p>{status}</p><p class="hint">M1 foundation: .cmap import and offline map rendering are not implemented yet.</p></div></main>
  <footer>Windows-first · Android tablet support planned · No network required for installed maps</footer>
</div>
