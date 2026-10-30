<script lang="ts">
  // The phone map viewer. It opens a .cmap file made by the main CindrixOps program (Maps > Send to phone) and shows
  // its street, topo and aerial pictures with the person's own pins, areas to avoid, drawings and directions.
  // The map drawing rules, military grid and shape arithmetic are the main program's own (copied in src/lib).
  import { onDestroy, onMount } from 'svelte';
  import 'maplibre-gl/dist/maplibre-gl.css';
  import type { Map as MLMap, Marker, StyleSpecification } from 'maplibre-gl';
  import { mapBase, openCmap, pickCmap, plainError, startupFile, type CmapInfo } from '../lib/cmap';
  import { toDms, toMgrs } from '../lib/coords';
  import { drawingFeatures, fmtArea, fmtLength, labelPoint, measureOf } from '../lib/mapdraw';
  import { readOverlays, routeLine, type Overlays } from '../lib/overlays';

  type Kind = 'street' | 'topo' | 'aerial';
  const KIND_NAMES: Record<Kind, string> = { street: 'Street', topo: 'Topo', aerial: 'Aerial' };
  const KIND_ORDER: Kind[] = ['street', 'topo', 'aerial'];

  let info: CmapInfo | null = $state(null);
  let overlays: Overlays | null = $state(null);
  let error = $state('');
  let busy = $state(false);
  let kind: Kind = $state('street');
  let mapOnly = $state(false);
  let panel = $state(false);
  let metric = $state(false);
  let centre: [number, number] = $state([0, 0]);
  let zoom = $state(0);
  let readout: 'mgrs' | 'dms' | 'deg' = $state('mgrs');
  let where = $state('');

  let el: HTMLDivElement | undefined = $state();
  let map: MLMap | null = null;
  let ml: typeof import('maplibre-gl') | null = null;
  let markers: Marker[] = [];
  let me: Marker | null = null;
  let watch: number | null = null;
  let vecIds: { id: string; type: string }[] = [];
  let rasterIds: Partial<Record<Kind, string>> = {};

  const kinds = $derived(info ? KIND_ORDER.filter((k) => info!.layers.some((l) => l.kind === k)) : []);

  // ---------- opening a file ----------
  async function load(path: string) {
    busy = true;
    error = '';
    try {
      const i = await openCmap(path);
      teardown();
      info = i;
      overlays = readOverlays(i.map);
      kind = kinds.includes('street') ? 'street' : (KIND_ORDER.find((k) => i.layers.some((l) => l.kind === k)) ?? 'street');
      if (i.unsupported.length) {
        error = 'Part of this map (' + i.unsupported.map((k) => KIND_NAMES[k as Kind] ?? k).join(', ') + ') uses a kind of packing this viewer cannot open yet. The rest is shown.';
      }
    } catch (e) {
      error = plainError(e);
    } finally {
      busy = false;
    }
  }

  async function choose() {
    try {
      const p = await pickCmap();
      if (p) await load(p);
    } catch (e) {
      error = plainError(e);
    }
  }

  onMount(async () => {
    try {
      const p = await startupFile();
      if (p) await load(p);
    } catch { /* a browser preview has no program behind it */ }
  });

  // draw the map once the file is open and the box exists
  $effect(() => {
    if (info && el && !map) void start(info, el);
  });

  // ---------- the map ----------
  async function start(i: CmapInfo, box: HTMLDivElement) {
    try {
      const [maplibregl, { layers, namedFlavor }] = await Promise.all([import('maplibre-gl'), import('@protomaps/basemaps')]);
      ml = maplibregl;
      const base = mapBase();
      const hasLetters = i.glyphs.length > 0;
      const usable = i.layers.filter((l) => !i.unsupported.includes(l.kind));
      if (!usable.length) {
        error = 'This map file holds only pictures this viewer cannot open yet.';
        return;
      }
      const sources: Record<string, unknown> = {};
      const rasters: unknown[] = [];
      rasterIds = {};
      for (const l of usable) {
        const url = base + 'tiles/' + l.kind + '/{z}/{x}/{y}';
        if (l.kind === 'street') {
          sources.street = { type: 'vector', tiles: [url], minzoom: l.min_zoom, maxzoom: l.max_zoom, attribution: '© OpenStreetMap contributors' };
        } else if (l.kind === 'topo' || l.kind === 'aerial') {
          sources[l.kind] = { type: 'raster', tiles: [url], tileSize: 256, minzoom: l.min_zoom, maxzoom: l.max_zoom, attribution: l.kind === 'topo' ? 'Topo: USGS The National Map' : 'Imagery: USGS The National Map' };
          rasters.push({ id: 'r-' + l.kind, type: 'raster', source: l.kind, layout: { visibility: 'none' } });
          rasterIds[l.kind as Kind] = 'r-' + l.kind;
        }
      }
      // street drawing rules: the same ones the main program uses; letters only when the file carries the fonts
      const keep = (l: { type: string; layout?: Record<string, unknown>; 'source-layer'?: string }) =>
        l.type !== 'symbol' || (hasLetters && l['source-layer'] !== 'pois' && !(l.layout && 'icon-image' in l.layout));
      const vec = (sources.street ? (layers('street', namedFlavor('light'), { lang: 'en' }) as unknown as { id: string; type: string }[]).filter((l) => keep(l as never)) : []) as { id: string; type: string }[];
      vecIds = vec.map((l) => ({ id: l.id, type: l.type }));
      const bg = vec.length ? [vec[0]] : [{ id: 'bg', type: 'background', paint: { 'background-color': '#e8e4d8' } }];
      const style = {
        version: 8,
        ...(hasLetters ? { glyphs: base + 'glyphs/{fontstack}/{range}.pbf' } : {}),
        sources,
        layers: [...bg, ...rasters, ...vec.slice(vec.length ? 1 : 0)],
      } as unknown as StyleSpecification;
      const [w, s, e, n] = i.bbox;
      const lowest = Math.min(...usable.map((l) => l.min_zoom));
      const m = new maplibregl.Map({
        container: box,
        style,
        center: [i.view.center[1], i.view.center[0]],
        zoom: i.view.zoom,
        minZoom: Math.max(0, lowest - 1),
        maxZoom: 22,
        attributionControl: { compact: true },
        pixelRatio: window.devicePixelRatio || 1,
        fadeDuration: 0,
        maxTileCacheSize: 200,
        // a map cut for one area has nothing beyond its edge: stop at the edge
        maxBounds: [[w - 0.1, s - 0.1], [e + 0.1, n + 0.1]],
      });
      map = m;
      m.addControl(new maplibregl.ScaleControl({ unit: metric ? 'metric' : 'imperial' }), 'bottom-left');
      m.on('error', (ev) => {
        const e = ev as unknown as { error?: { status?: number }; tile?: unknown };
        if (e.error?.status === 404) return; // a square the map does not hold, or a missing letter: not a broken map
        if (e.tile) error = 'Some of the map could not be read: the file may be damaged.';
      });
      const sync = () => {
        const c = m.getCenter();
        centre = [c.lat, c.lng];
        zoom = m.getZoom();
      };
      m.on('move', sync);
      m.on('load', () => {
        sync();
        wireOverlays(m);
        applyKind();
      });
      const resize = () => m.resize();
      window.addEventListener('resize', resize);
      window.addEventListener('orientationchange', resize);
      stopResize = () => {
        window.removeEventListener('resize', resize);
        window.removeEventListener('orientationchange', resize);
      };
    } catch (err) {
      error = 'The map could not be drawn: ' + plainError(err);
    }
  }
  let stopResize: (() => void) | null = null;

  const lonlat = (p: [number, number]): [number, number] => [p[1], p[0]];
  const empty = () => ({ type: 'FeatureCollection' as const, features: [] as unknown[] });

  function wireOverlays(m: MLMap) {
    const o = overlays;
    if (!o || !ml) return;
    for (const id of ['route', 'nogo', 'draw', 'dir']) m.addSource(id, { type: 'geojson', data: empty() as never });
    m.addLayer({ id: 'nogo-fill', type: 'fill', source: 'nogo', paint: { 'fill-color': '#e5533d', 'fill-opacity': 0.25 } });
    m.addLayer({ id: 'nogo-line', type: 'line', source: 'nogo', paint: { 'line-color': '#e5533d', 'line-width': 2, 'line-dasharray': [2, 2] } });
    m.addLayer({ id: 'draw-fill', type: 'fill', source: 'draw', filter: ['==', ['geometry-type'], 'Polygon'], paint: { 'fill-color': ['get', 'color'], 'fill-opacity': 0.22 } });
    m.addLayer({ id: 'draw-line', type: 'line', source: 'draw', layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': ['get', 'color'], 'line-width': ['get', 'width'] } });
    m.addLayer({ id: 'route-line', type: 'line', source: 'route', layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': '#1a73e8', 'line-width': 4, 'line-dasharray': [1, 1.5] } });
    m.addLayer({ id: 'dir-casing', type: 'line', source: 'dir', layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': '#ffffff', 'line-width': 9 } });
    m.addLayer({ id: 'dir-line', type: 'line', source: 'dir', layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': '#1a73e8', 'line-width': 5.5 } });
    const src = (id: string, data: unknown) => (m.getSource(id) as { setData: (d: unknown) => void } | undefined)?.setData(data);
    const rp = routeLine(o);
    src('route', rp.length > 1 && !o.directions ? { type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: rp.map(lonlat) } } : empty());
    src('nogo', {
      type: 'FeatureCollection',
      features: o.nogo.map((a) => ({ type: 'Feature', properties: {}, geometry: { type: 'Polygon', coordinates: [[...a.points, a.points[0]].map(lonlat)] } })),
    });
    src('draw', drawingFeatures(o.drawings, null));
    src('dir', o.directions ? { type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: o.directions.points.map(lonlat) } } : empty());
    // pins and names are page elements: text is set as text, never as markup
    for (const p of o.pins) {
      const d = document.createElement('div');
      d.className = 'pinmark';
      d.innerHTML = '<span class="pdot"></span><span class="plab"></span>';
      (d.querySelector('.pdot') as HTMLElement).style.background = p.color;
      (d.querySelector('.plab') as HTMLElement).textContent = p.label;
      markers.push(new ml.Marker({ element: d, anchor: 'left' }).setLngLat([p.lon, p.lat]).addTo(m));
    }
    for (const dr of o.drawings) {
      const at = labelPoint(dr);
      if (!at || !dr.label) continue;
      const d = document.createElement('div');
      d.className = dr.kind === 'text' ? 'dtext' : 'dname';
      d.textContent = dr.label;
      d.style.color = dr.kind === 'text' ? dr.color : '#202124';
      markers.push(new ml.Marker({ element: d, anchor: 'center' }).setLngLat(lonlat(at)).addTo(m));
    }
  }

  /** Show the chosen picture; street lines and names stay on top of a topo or aerial picture. */
  function applyKind() {
    const m = map;
    if (!m) return;
    for (const k of ['topo', 'aerial'] as Kind[]) {
      const id = rasterIds[k];
      if (id && m.getLayer(id)) m.setLayoutProperty(id, 'visibility', kind === k ? 'visible' : 'none');
    }
    for (const l of vecIds) {
      if (!m.getLayer(l.id)) continue;
      const hide = kind !== 'street' && (l.type === 'fill' || l.type === 'background');
      m.setLayoutProperty(l.id, 'visibility', hide ? 'none' : 'visible');
    }
  }
  $effect(() => {
    void kind;
    applyKind();
  });

  // ---------- going to things ----------
  function flyTo(lat: number, lon: number) {
    map?.flyTo({ center: [lon, lat], zoom: Math.max(map.getZoom(), 14), duration: 500 });
    if (window.matchMedia('(max-width: 700px)').matches) panel = false;
  }
  function wholeMap() {
    if (!map || !info) return;
    const [w, s, e, n] = info.bbox;
    map.fitBounds([[w, s], [e, n]], { padding: 24, duration: 400 });
  }

  // ---------- where I am ----------
  function locate() {
    if (!map || !ml) return;
    if (!navigator.geolocation) {
      where = 'This device cannot tell where it is.';
      return;
    }
    if (watch !== null) {
      navigator.geolocation.clearWatch(watch);
      watch = null;
      me?.remove();
      me = null;
      where = '';
      return;
    }
    where = 'Looking for your position…';
    let first = true;
    watch = navigator.geolocation.watchPosition(
      (pos) => {
        if (!map || !ml) return;
        const ll: [number, number] = [pos.coords.longitude, pos.coords.latitude];
        if (!me) {
          const d = document.createElement('div');
          d.className = 'me';
          me = new ml.Marker({ element: d }).setLngLat(ll).addTo(map);
        } else {
          me.setLngLat(ll);
        }
        where = 'Accurate to about ' + Math.round(pos.coords.accuracy * (metric ? 1 : 3.281)) + (metric ? ' m' : ' ft');
        if (first) {
          first = false;
          map.flyTo({ center: ll, zoom: Math.max(map.getZoom(), 15), duration: 500 });
        }
      },
      (err) => {
        where = err.code === 1 ? 'Location is switched off for this app. Turn it on in the device settings.' : 'Your position could not be found yet.';
        watch = null;
      },
      { enableHighAccuracy: true, maximumAge: 5000, timeout: 30000 },
    );
  }

  // ---------- full screen ----------
  async function toggleFull() {
    mapOnly = !mapOnly;
    if (mapOnly) {
      panel = false;
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        await getCurrentWindow().setFullscreen(true);
      } catch { /* phones are already full screen */ }
    }
    setTimeout(() => map?.resize(), 50);
  }

  function teardown() {
    stopResize?.();
    stopResize = null;
    if (watch !== null && navigator.geolocation) navigator.geolocation.clearWatch(watch);
    watch = null;
    for (const k of markers) k.remove();
    markers = [];
    me?.remove();
    me = null;
    map?.remove();
    map = null;
    where = '';
  }
  onDestroy(teardown);

  const readText = $derived.by(() => {
    if (readout === 'mgrs') return toMgrs(centre[0], centre[1], 4) ?? toDms(centre[0], centre[1]);
    if (readout === 'dms') return toDms(centre[0], centre[1]);
    return centre[0].toFixed(5) + ', ' + centre[1].toFixed(5);
  });
  const cycle = () => (readout = readout === 'mgrs' ? 'dms' : readout === 'dms' ? 'deg' : 'mgrs');
  const mins = (m: number) => (m < 60 ? Math.round(m) + ' min' : Math.floor(m / 60) + ' h ' + Math.round(m % 60) + ' min');
  const counts = $derived(overlays ? overlays.pins.length + overlays.nogo.length + overlays.drawings.length + (overlays.directions ? 1 : 0) : 0);
</script>

{#if !info}
  <section class="start">
    <div class="symbol" aria-hidden="true">⌖</div>
    <h2>Open a phone map</h2>
    <p>Choose a <strong>.cmap</strong> file. Make one in CindrixOps: <em>Maps › Send to phone</em>, then copy the file to this device.</p>
    <button class="primary" onclick={choose} disabled={busy} title="Choose a phone map file on this device">{busy ? 'Opening…' : 'Open a map file'}</button>
    {#if error}<p class="problem" role="alert">{error}</p>{/if}
  </section>
{:else}
  <section class="viewer" class:maponly={mapOnly}>
    {#if !mapOnly}
      <div class="bar">
        <button class="ic" onclick={choose} title="Open another map file" aria-label="Open another map file">
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
        </button>
        <div class="name" title={info.name}>{info.name}</div>
        {#if kinds.length > 1}
          <div class="seg" role="group" aria-label="Kind of map">
            {#each kinds as k}
              <button class:on={kind === k} onclick={() => (kind = k)} title={'Show the ' + KIND_NAMES[k].toLowerCase() + ' map'}>{KIND_NAMES[k]}</button>
            {/each}
          </div>
        {/if}
        <button class="ic" class:on={panel} onclick={() => (panel = !panel)} title="Pins, areas, drawings and directions" aria-label="Pins, areas, drawings and directions">
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" /></svg>
          {#if counts}<span class="badge">{counts}</span>{/if}
        </button>
        <button class="ic" onclick={toggleFull} title="Make the map fill the whole screen" aria-label="Full screen map">
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" /></svg>
        </button>
      </div>
    {/if}
    {#if error}<div class="notice" role="alert">{error}<button class="x" onclick={() => (error = '')} aria-label="Dismiss">×</button></div>{/if}

    <div class="stage">
      <div class="map" bind:this={el}></div>

      <div class="float right">
        {#if mapOnly}
          <button class="ic solid" onclick={toggleFull} title="Show the buttons again" aria-label="Leave full screen map">
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" /></svg>
          </button>
        {/if}
        <button class="ic solid" class:on={watch !== null} onclick={locate} title="Show where I am" aria-label="Show where I am">
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M12 2v4M12 18v4M2 12h4M18 12h4" /></svg>
        </button>
        <button class="ic solid" onclick={wholeMap} title="Show the whole map" aria-label="Show the whole map">
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="2" /><path d="M4 12h16M12 4v16" /></svg>
        </button>
      </div>

      <div class="readout">
        <button onclick={cycle} title="Tap to change how the position is written">{readText}</button>
        <span>zoom {zoom.toFixed(1)}</span>
        {#if where}<span class="where">{where}</span>{/if}
      </div>

      {#if panel && overlays}
        <aside class="sheet" aria-label="Marks on this map">
          <div class="sheethead">
            <strong>On this map</strong>
            <button class="unit" onclick={() => (metric = !metric)} title="Switch between miles and kilometres">{metric ? 'km' : 'mi'}</button>
            <button class="x" onclick={() => (panel = false)} aria-label="Close the list">×</button>
          </div>
          {#if overlays.directions}
            <h3>Directions</h3>
            <p class="line">{fmtLength(overlays.directions.km, metric)} · {mins(overlays.directions.minutes)}</p>
            <ol class="steps">
              {#each overlays.directions.steps as s}<li>{s.name || 'Continue'} <span>{fmtLength(s.km, metric)}</span></li>{/each}
            </ol>
          {/if}
          {#if overlays.pins.length}
            <h3>Pins</h3>
            <ul>
              {#each overlays.pins as p}
                <li><button onclick={() => flyTo(p.lat, p.lon)}><span class="dot" style="background:{p.color}"></span>{p.label}{#if p.tag}<small>{p.tag}</small>{/if}</button></li>
              {/each}
            </ul>
          {/if}
          {#if overlays.nogo.length}
            <h3>Areas to avoid</h3>
            <ul>
              {#each overlays.nogo as a}
                <li><button onclick={() => flyTo(a.points.reduce((s, p) => s + p[0], 0) / a.points.length, a.points.reduce((s, p) => s + p[1], 0) / a.points.length)}><span class="dot" style="background:#e5533d"></span>{a.label}</button></li>
              {/each}
            </ul>
          {/if}
          {#if overlays.drawings.length}
            <h3>Drawings</h3>
            <ul>
              {#each overlays.drawings as d}
                {@const at = labelPoint(d)}
                {@const ms = measureOf(d)}
                <li><button onclick={() => at && flyTo(at[0], at[1])}><span class="dot" style="background:{d.color}"></span>{d.label || (d.kind === 'line' ? 'Line' : d.kind === 'text' ? 'Note' : 'Shape')}{#if ms.km2 > 0}<small>{fmtArea(ms.km2, metric)}</small>{:else if ms.km > 0}<small>{fmtLength(ms.km, metric)}</small>{/if}</button></li>
              {/each}
            </ul>
          {/if}
          {#if overlays.notes}
            <h3>Notes</h3>
            <p class="notes">{overlays.notes}</p>
          {/if}
          {#if !counts && !overlays.notes}<p class="line">Nothing was added to this map.</p>{/if}
        </aside>
      {/if}
    </div>
  </section>
{/if}

<style>
  .start { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; text-align: center; padding: 24px; min-height: 100%; }
  .start .symbol { font-size: 72px; color: var(--accent); line-height: 1; }
  .start h2 { margin: 0; font-size: 24px; color: #fff; }
  .start p { margin: 0; max-width: 34ch; color: #c4cbd0; line-height: 1.45; }
  .primary { background: var(--accent); color: #11171c; font-weight: 600; min-height: 48px; padding: 12px 22px; font-size: 16px; }
  .primary:disabled { opacity: 0.6; }
  .problem { color: #ffb4a8; }

  .viewer { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .bar { display: flex; align-items: center; gap: 6px; padding: 6px 8px; background: #181e24; border-bottom: 1px solid var(--line2); }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .ic { position: relative; display: inline-grid; place-items: center; width: 44px; height: 44px; padding: 0; border-radius: 8px; flex: none; }
  .ic.on { background: rgba(215, 181, 109, 0.25); }
  .ic.solid { background: #181e24; box-shadow: 0 1px 6px rgba(0, 0, 0, 0.5); }
  .badge { position: absolute; top: 2px; right: 2px; min-width: 16px; height: 16px; padding: 0 4px; border-radius: 8px; background: var(--accent); color: #11171c; font-size: 11px; line-height: 16px; font-weight: 700; }
  .seg { display: flex; border: 1px solid var(--accent); border-radius: 8px; overflow: hidden; flex: none; }
  .seg button { border: 0; border-radius: 0; min-height: 44px; padding: 0 12px; }
  .seg button.on { background: var(--accent); color: #11171c; font-weight: 600; }

  .notice { display: flex; align-items: center; gap: 8px; padding: 8px 12px; background: #4a2b25; color: #ffd9d2; font-size: 14px; }
  .x { margin-left: auto; border: 0; min-width: 36px; min-height: 36px; padding: 0; font-size: 22px; line-height: 1; }

  .stage { position: relative; flex: 1; min-height: 0; }
  .map { position: absolute; inset: 0; }
  .float { position: absolute; top: 10px; right: 10px; display: flex; flex-direction: column; gap: 8px; z-index: 5; }
  .readout { position: absolute; left: 8px; bottom: 34px; right: 60px; display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; z-index: 4; pointer-events: none; }
  .readout button { pointer-events: auto; min-height: 36px; padding: 4px 10px; background: rgba(17, 23, 28, 0.85); border-color: var(--line2); font-variant-numeric: tabular-nums; font-size: 14px; }
  .readout span { background: rgba(17, 23, 28, 0.85); padding: 4px 8px; border-radius: 6px; font-size: 12px; color: var(--muted); }
  .readout .where { color: #f5e8c7; }

  .sheet { position: absolute; z-index: 6; background: var(--panel); border: 1px solid var(--line2); color: var(--text); overflow: auto; padding: 8px 12px 16px; left: 0; right: 0; bottom: 0; max-height: 58%; border-radius: 14px 14px 0 0; box-shadow: 0 -4px 20px rgba(0, 0, 0, 0.5); }
  .sheethead { display: flex; align-items: center; gap: 8px; position: sticky; top: -8px; background: var(--panel); padding: 8px 0; }
  .sheethead strong { flex: 1; }
  .unit { min-height: 36px; min-width: 44px; padding: 0 10px; }
  .sheet h3 { margin: 14px 0 4px; font-size: 12px; letter-spacing: 1px; text-transform: uppercase; color: var(--muted); }
  .sheet ul { list-style: none; margin: 0; padding: 0; }
  .sheet li button { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 44px; text-align: left; border: 0; border-bottom: 1px solid var(--line2); border-radius: 0; padding: 6px 4px; color: var(--text); }
  .sheet li small { margin-left: auto; color: var(--muted); font-size: 12px; letter-spacing: 0; }
  .dot { width: 12px; height: 12px; border-radius: 50%; border: 2px solid #0f1110; flex: none; }
  .line { margin: 4px 0; }
  .steps { margin: 4px 0 0; padding-left: 20px; }
  .steps li { padding: 4px 0; }
  .steps li span { color: var(--muted); margin-left: 6px; font-size: 12px; }
  .notes { white-space: pre-wrap; margin: 4px 0; line-height: 1.4; }

  /* wider screens: the list sits at the side instead of rising from the bottom */
  @media (min-width: 701px) {
    .sheet { left: auto; top: 0; bottom: 0; width: 340px; max-height: none; border-radius: 0; border-width: 0 0 0 1px; box-shadow: -4px 0 20px rgba(0, 0, 0, 0.4); }
    .float { right: 10px; }
    .stage:has(.sheet) .float { right: 350px; }
  }
  /* small phones: keep the top bar on one line */
  @media (max-width: 420px) {
    .bar { gap: 4px; padding: 4px 6px; }
    .seg button { padding: 0 8px; font-size: 13px; }
    .name { font-size: 14px; }
    .readout { right: 56px; }
  }

  :global(.pinmark) { display: flex; align-items: center; gap: 6px; pointer-events: none; }
  :global(.pinmark .pdot) { width: 14px; height: 14px; border-radius: 50%; border: 2px solid #0f1110; display: inline-block; }
  :global(.pinmark .plab) { color: #ece7de; font-size: 13px; text-shadow: 0 0 3px #000, 0 0 3px #000; white-space: nowrap; }
  :global(.dtext) { font: 700 15px system-ui, sans-serif; text-shadow: 0 0 3px #fff, 0 0 3px #fff, 0 0 5px #fff; white-space: nowrap; padding: 2px 4px; pointer-events: none; }
  :global(.dname) { font: 600 12px system-ui, sans-serif; text-shadow: 0 0 3px #fff, 0 0 3px #fff; white-space: nowrap; pointer-events: none; }
  :global(.me) { width: 18px; height: 18px; border-radius: 50%; background: #1a73e8; border: 3px solid #fff; box-shadow: 0 0 0 4px rgba(26, 115, 232, 0.3); }
</style>
