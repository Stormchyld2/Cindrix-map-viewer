<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { mapsList, mapLoad, mapSave, mapDelete, mapExportGpx, routeStats, mapPacks, tileUrl, placeSearch, type PlaceHit, type MapMeta, type LatLon, type RouteStats, type Pack } from '../lib/maps-api';
  import type { Map as MLMap, Marker, StyleSpecification, GeoJSONSource } from 'maplibre-gl';
  import { settings } from '../lib/map-settings';
  import { POI_GROUPS, kindName, groupOf, distanceKm, compass } from '../lib/mapkinds';

  type Pin = { id: number; label: string; kind: 'pin' | 'rendezvous'; lat: number; lon: number };
  type Area = { label: string; points: LatLon[] };
  type MapFile = { version: 1; name: string; pins: Pin[]; route: number[]; nogo: Area[] };

  let saved: MapMeta[] = $state([]);
  let current: string | null = $state(null); // id of the saved map being edited, null for a new one
  let name = $state('');
  let notes = $state('');
  let pins: Pin[] = $state([]);
  let route: number[] = $state([]); // pin ids in order
  let nogo: Area[] = $state([]);
  let draft: LatLon[] = $state([]); // no-go area being drawn
  let draftLabel = $state('');
  let message = $state('');
  let error = $state('');
  /** What the open map file covers, read from its header. */
  let packInfo = $state('');
  /** True when the map file covers only part of the country: the grey beyond its edge is simply outside it. */
  let partial = $state(false);
  let stats: RouteStats | null = $state(null);
  let confirmDelete = $state(false);

  // form fields
  let pLabel = $state('');
  let pKind: 'pin' | 'rendezvous' = $state('pin');
  let pLat = $state('');
  let pLon = $state('');
  let aLat = $state('');
  let aLon = $state('');

  // ---------- the tile map (only when a street pack is on the drive) ----------
  let packs: Pack[] = $state([]);
  let street: Pack | undefined = $derived(packs.find((p) => p.kind === 'street'));
  let mapEl: HTMLDivElement | undefined = $state();
  let mapReady = $state(false);
  let tileError = $state('');
  let addMode: 'off' | 'pin' | 'rendezvous' | 'corner' = $state('off');
  let map: MLMap | null = null;
  let resizer: ResizeObserver | null = null;
  let startBox: { box: [[number, number], [number, number]]; maxZoom: number } | null = null;
  let markers: Marker[] = [];
  // full screen, cursor readout, centre cross-hair
  let stageEl: HTMLDivElement | undefined = $state();
  let full = $state(false);
  let apiFull = false;
  let cursor: LatLon | null = $state(null);
  let centre: LatLon | null = $state(null);
  let cross = $state(true);
  let drawer = $state(true);
  // places marked on the map (hospitals, water, fuel ...), switched on and off by group
  let layersOpen = $state(false);
  let labelsOn = $state(false); // place names are drawn only when the font files are on the drive
  let poiOn: Record<string, boolean> = $state(loadPoiChoice());
  type PoiPick = { name: string; kind: string; lat: number; lon: number };
  let picked: PoiPick | null = $state(null);
  let nearGroup = $state('emergency');
  let nearList: (PoiPick & { km: number; dir: string })[] = $state([]);
  let nearNote = $state('');

  function loadPoiChoice(): Record<string, boolean> {
    const base: Record<string, boolean> = Object.fromEntries(POI_GROUPS.map((g) => [g.id, g.on]));
    try {
      const raw = localStorage.getItem('cindrix-map-layers');
      if (raw) {
        const v = JSON.parse(raw) as Record<string, unknown>;
        for (const g of POI_GROUPS) if (typeof v[g.id] === 'boolean') base[g.id] = v[g.id] as boolean;
      }
    } catch { /* the choice is just not remembered */ }
    return base;
  }
  function savePoiChoice() {
    try { localStorage.setItem('cindrix-map-layers', JSON.stringify(poiOn)); } catch { /* not remembered */ }
  }
  function setGroup(id: string, on: boolean) {
    poiOn[id] = on;
    savePoiChoice();
    applyPoiVisibility();
  }
  function setAllGroups(on: boolean) {
    for (const g of POI_GROUPS) poiOn[g.id] = on;
    savePoiChoice();
    applyPoiVisibility();
  }
  function applyPoiVisibility() {
    if (!map || !mapReady) return;
    for (const g of POI_GROUPS) {
      const v = poiOn[g.id] ? 'visible' : 'none';
      for (const id of [`poi-${g.id}`, `poi-${g.id}-label`]) if (map.getLayer(id)) map.setLayoutProperty(id, 'visibility', v);
    }
  }
  function addPoiLayers(m: MLMap, withNames: boolean) {
    for (const g of POI_GROUPS) {
      const filter = ['match', ['get', 'kind'], g.kinds, true, false];
      const vis = poiOn[g.id] ? 'visible' : 'none';
      m.addLayer({
        id: `poi-${g.id}`, type: 'circle', source: 'protomaps', 'source-layer': 'pois', minzoom: 7, filter,
        layout: { visibility: vis },
        paint: { 'circle-color': g.color, 'circle-radius': ['interpolate', ['linear'], ['zoom'], 7, 2.5, 12, 5, 16, 8], 'circle-stroke-color': '#0b0d0c', 'circle-stroke-width': 1.5 },
      } as never);
      if (withNames) {
        m.addLayer({
          id: `poi-${g.id}-label`, type: 'symbol', source: 'protomaps', 'source-layer': 'pois', minzoom: 12.5, filter,
          layout: { visibility: vis, 'text-field': ['coalesce', ['get', 'name'], ''], 'text-font': ['Noto Sans Regular'], 'text-size': 11, 'text-anchor': 'top', 'text-offset': [0, 0.8], 'text-optional': true, 'text-max-width': 8 },
          paint: { 'text-color': '#ece7de', 'text-halo-color': '#0b0d0c', 'text-halo-width': 1.4 },
        } as never);
      }
    }
  }
  /** Is there a marked place under this point of the screen? */
  function poiAt(m: MLMap, point: { x: number; y: number }): PoiPick | null {
    const layers = POI_GROUPS.filter((g) => poiOn[g.id] && m.getLayer(`poi-${g.id}`)).map((g) => `poi-${g.id}`);
    if (!layers.length) return null;
    const r = 8;
    const f = m.queryRenderedFeatures([[point.x - r, point.y - r], [point.x + r, point.y + r]], { layers })[0];
    if (!f || f.geometry.type !== 'Point') return null;
    const [lon, lat] = f.geometry.coordinates as [number, number];
    const kind = String(f.properties?.kind ?? '');
    return { name: String(f.properties?.name ?? '') || kindName(kind), kind, lat, lon };
  }
  function findNearest() {
    nearNote = '';
    nearList = [];
    if (!map || !mapReady) return;
    const g = POI_GROUPS.find((x) => x.id === nearGroup);
    if (!g) return;
    const c = map.getCenter();
    const from: [number, number] = [c.lat, c.lng];
    const feats = map.querySourceFeatures('protomaps', { sourceLayer: 'pois', filter: ['match', ['get', 'kind'], g.kinds, true, false] as never });
    const seen = new Set<string>();
    const out: (PoiPick & { km: number; dir: string })[] = [];
    for (const f of feats) {
      if (f.geometry.type !== 'Point') continue;
      const [lon, lat] = f.geometry.coordinates as [number, number];
      const kind = String(f.properties?.kind ?? '');
      const name = String(f.properties?.name ?? '') || kindName(kind);
      const key = `${name}|${lat.toFixed(4)}|${lon.toFixed(4)}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ name, kind, lat, lon, km: distanceKm(from, [lat, lon]), dir: compass(from, [lat, lon]) });
    }
    out.sort((a, b) => a.km - b.km);
    nearList = out.slice(0, 8);
    nearNote = out.length
      ? 'Nearest to the middle of the map. This looks only at the part of the map on screen, so zoom out to look further.'
      : 'None found on the part of the map on screen. Zoom out or move the map and try again.';
  }
  function showPick(h: PoiPick) {
    if (!map) return;
    map.jumpTo({ center: [h.lon, h.lat], zoom: Math.max(map.getZoom(), 14) });
    picked = h;
  }
  function pinPick(h: PoiPick) {
    const id = pins.reduce((a, x) => Math.max(a, x.id), 0) + 1;
    pins = [...pins, { id, label: h.name, kind: 'pin', lat: Math.round(h.lat * 1e5) / 1e5, lon: Math.round(h.lon * 1e5) / 1e5 }];
  }
  const dist = (km: number) => (settings.values.mapUnits === 'metric' ? `${km.toFixed(km < 10 ? 1 : 0)} km` : `${(km * 0.621371).toFixed(km * 0.621371 < 10 ? 1 : 0)} mi`);

  function toggleFull() {
    full = !full;
    if (full) {
      // Also ask the window to hide its own frame. If that is refused the map still fills the app.
      try { void stageEl?.requestFullscreen?.().then(() => (apiFull = true)).catch(() => (apiFull = false)); } catch { apiFull = false; }
    } else if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => {});
      apiFull = false;
    }
    queueMicrotask(() => map?.resize());
    setTimeout(() => map?.resize(), 150);
  }
  function onFullChange() {
    if (apiFull && !document.fullscreenElement) { full = false; apiFull = false; setTimeout(() => map?.resize(), 100); }
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && full) { e.preventDefault(); toggleFull(); }
  }
  let ml: typeof import('maplibre-gl') | null = null;
  let protocol: import('pmtiles').Protocol | null = null;

  onMount(async () => {
    document.addEventListener('fullscreenchange', onFullChange);
    await refresh();
    try {
      packs = await mapPacks();
    } catch (e) {
      tileError = String(e);
    }
  });
  onDestroy(() => {
    document.removeEventListener('fullscreenchange', onFullChange);
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    resizer?.disconnect();
    map?.remove();
    map = null;
  });

  $effect(() => {
    if (street && mapEl && !map) void initMap(street, mapEl);
  });

  async function initMap(pack: Pack, el: HTMLDivElement) {
    try {
      const [maplibregl, { Protocol, PMTiles }, { layers, namedFlavor }] = await Promise.all([
        import('maplibre-gl'),
        import('pmtiles'),
        import('@protomaps/basemaps'),
      ]);
      await import('maplibre-gl/dist/maplibre-gl.css');
      ml = maplibregl;
      if (!protocol) {
        protocol = new Protocol();
        // The cast keeps this compiling whichever handler signature the two libraries agree on.
        maplibregl.addProtocol('pmtiles', protocol.tile as never);
      }
      const url = await tileUrl(pack.rel);
      // Read the file's header first: it says which area and zoom levels the pack really holds.
      // A pack cut for one town has nothing to show at the whole-country view the map used to open on.
      const archive = new PMTiles(url);
      protocol.add(archive);
      let area: { w: number; s: number; e: number; n: number; minZoom: number; maxZoom: number } | null = null;
      try {
        const h = await archive.getHeader();
        if ([h.minLon, h.minLat, h.maxLon, h.maxLat].every((v) => Number.isFinite(v)) && h.maxLon > h.minLon && h.maxLat > h.minLat) {
          area = { w: h.minLon, s: h.minLat, e: h.maxLon, n: h.maxLat, minZoom: h.minZoom, maxZoom: h.maxZoom };
          packInfo = `${pack.name}: covers ${h.minLat.toFixed(2)}, ${h.minLon.toFixed(2)} to ${h.maxLat.toFixed(2)}, ${h.maxLon.toFixed(2)}; zoom ${h.minZoom} to ${h.maxZoom}.`;
        }
      } catch (e) {
        tileError = 'The map file could not be read: ' + String((e as Error)?.message ?? e) + ' The file may be damaged or incomplete; cut it again with scripts\\get-maps.ps1.';
        return;
      }
      // No glyph (font) files ship yet, so the text layers are left out: roads, water, land and
      // buildings are drawn, place names are not. Pin labels are drawn by this screen instead.
      // Place names need the font files in maps/fonts. When they are there the lettering is drawn (street, town and
      // water names, and the names of marked places); when not, only shapes are drawn and the map says how to add them.
      let glyphs: string | undefined;
      try {
        const tpl = (await tileUrl('fonts/FONTSTACKX/FONTRANGEX.pbf')).replace('FONTSTACKX', '{fontstack}').replace('FONTRANGEX', '{range}');
        const probe = tpl.replace('{fontstack}', encodeURIComponent('Noto Sans Regular')).replace('{range}', '0-255');
        const r = await fetch(probe);
        if (r.ok && (await r.arrayBuffer()).byteLength > 0) glyphs = tpl;
      } catch { /* no fonts: shapes only */ }
      labelsOn = !!glyphs;
      // symbol layers that use picture icons are left out (no icon sheet ships); text-only ones are kept when the fonts are there
      const keep = (l: { type: string; layout?: Record<string, unknown>; 'source-layer'?: string }) =>
        l.type !== 'symbol' || (labelsOn && l['source-layer'] !== 'pois' && !(l.layout && 'icon-image' in l.layout));
      const style = {
        version: 8,
        ...(glyphs ? { glyphs } : {}),
        sources: { protomaps: { type: 'vector', url: 'pmtiles://' + url, attribution: '© OpenStreetMap contributors' } },
        layers: layers('protomaps', namedFlavor('black'), { lang: 'en' }).filter((l) => keep(l as never)),
      } as unknown as StyleSpecification;
      const m = new maplibregl.Map({
        container: el,
        style,
        center: area ? [(area.w + area.e) / 2, (area.s + area.n) / 2] : [-98.5, 39.8],
        zoom: area ? Math.max(area.minZoom, 3) : 3.5,
        attributionControl: { compact: true },
        fadeDuration: 0,
        maxTileCacheSize: 200,
      });
      // Open on the United States when the pack covers any of it (a world pack would otherwise open on the whole
      // globe); a pack cut for one town or one country elsewhere opens on what it covers.
      if (area) {
        const us = { w: -125, s: 24, e: -66.5, n: 49.5 };
        const w = Math.max(area.w, us.w), s = Math.max(area.s, us.s), e = Math.min(area.e, us.e), n = Math.min(area.n, us.n);
        const [bw, bs, be, bn] = e > w && n > s ? [w, s, e, n] : [area.w, area.s, area.e, area.n];
        startBox = { box: [[bw, bs], [be, bn]], maxZoom: Math.min(area.maxZoom, 15) };
        m.fitBounds(startBox.box, { padding: 20, duration: 0, maxZoom: startBox.maxZoom });
      }
      // A map file cut for one area has nothing beyond its edge, which shows as grey. Stop the map at the edge.
      if (area && (area.e - area.w < 40 || area.n - area.s < 20)) {
        partial = true;
        m.setMaxBounds([[area.w - 0.2, area.s - 0.2], [area.e + 0.2, area.n + 0.2]]);
      }
      // The map must follow its box when the window or the interface size changes, or part of it stays grey.
      if (typeof ResizeObserver !== 'undefined') {
        resizer = new ResizeObserver(() => m.resize());
        resizer.observe(el);
      }
      m.addControl(new maplibregl.NavigationControl({ showCompass: true }), 'top-right');
      m.addControl(new maplibregl.ScaleControl({ unit: settings.values.mapUnits === 'metric' ? 'metric' : 'imperial' }), 'bottom-left');
      m.on('error', (e) => {
        const msg = String((e as { error?: Error }).error?.message ?? e);
        if (/image|sprite|glyph|font/i.test(msg) && !/tile/i.test(msg)) return; // a missing picture or letter is not a broken map
        tileError = 'The map could not read its tiles: ' + String((e as { error?: Error }).error?.message ?? e);
      });
      m.on('load', () => {
        for (const id of ['route', 'nogo', 'draft']) m.addSource(id, { type: 'geojson', data: empty() });
        m.addLayer({ id: 'nogo-fill', type: 'fill', source: 'nogo', paint: { 'fill-color': '#e5533d', 'fill-opacity': 0.25 } });
        m.addLayer({ id: 'nogo-line', type: 'line', source: 'nogo', paint: { 'line-color': '#e5533d', 'line-width': 2, 'line-dasharray': [2, 2] } });
        m.addLayer({ id: 'draft-line', type: 'line', source: 'draft', paint: { 'line-color': '#e5533d', 'line-width': 2, 'line-dasharray': [1, 2] } });
        addPoiLayers(m, labelsOn);
        m.addLayer({ id: 'route-line', type: 'line', source: 'route', paint: { 'line-color': '#ff9a1f', 'line-width': 4 } });
        mapReady = true;
        // The box may have settled to its real size after the map was made: measure again, then frame the start view.
        m.resize();
        if (startBox && pins.length === 0 && nogo.length === 0) m.fitBounds(startBox.box, { padding: 20, duration: 0, maxZoom: startBox.maxZoom });
        zoomToPoints();
      });
      m.on('click', (e) => {
        // with nothing to add, a click on a marked place opens its card
        if (addMode === 'off') {
          const hit = poiAt(m, e.point);
          if (hit) { picked = hit; return; }
          picked = null;
        }
        onMapClick(e.lngLat.lat, e.lngLat.lng);
      });
      m.on('mousemove', (e) => { cursor = [Math.round(e.lngLat.lat * 1e5) / 1e5, Math.round(e.lngLat.lng * 1e5) / 1e5]; });
      m.on('mouseout', () => { cursor = null; });
      const setCentre = () => { const c = m.getCenter(); centre = [Math.round(c.lat * 1e5) / 1e5, Math.round(c.lng * 1e5) / 1e5]; };
      m.on('move', setCentre);
      setCentre();
      map = m;
    } catch (e) {
      tileError = 'The map viewer could not start: ' + String(e);
    }
  }

  const empty = () => ({ type: 'FeatureCollection' as const, features: [] });
  const ring = (pts: LatLon[]) => [...pts, pts[0]].map(([la, lo]) => [lo, la]);

  function onMapClick(lat: number, lon: number) {
    const p: LatLon = [Math.round(lat * 1e5) / 1e5, Math.round(lon * 1e5) / 1e5];
    if (addMode === 'pin' || addMode === 'rendezvous') {
      const id = pins.reduce((m, x) => Math.max(m, x.id), 0) + 1;
      pins = [...pins, { id, label: (addMode === 'rendezvous' ? 'Rendezvous ' : 'Pin ') + id, kind: addMode, lat: p[0], lon: p[1] }];
    } else if (addMode === 'corner') {
      draft = [...draft, p];
    }
  }

  function zoomToPoints() {
    if (!map || !ml) return;
    const all: LatLon[] = [...pins.map((p) => [p.lat, p.lon] as LatLon), ...nogo.flatMap((a) => a.points)];
    if (all.length === 0) return;
    const b = new ml.LngLatBounds([all[0][1], all[0][0]], [all[0][1], all[0][0]]);
    for (const [la, lo] of all) b.extend([lo, la]);
    map.fitBounds(b, { padding: 60, maxZoom: 15, duration: 0 });
  }

  // Keep the drawn layers and the pin markers in step with what is on screen.
  $effect(() => {
    if (!map || !ml || !mapReady) return;
    const m = map;
    const rp = routePoints;
    (m.getSource('route') as GeoJSONSource).setData(
      rp.length > 1
        ? { type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: rp.map(([la, lo]) => [lo, la]) } }
        : empty(),
    );
    (m.getSource('nogo') as GeoJSONSource).setData({
      type: 'FeatureCollection',
      features: nogo.filter((a) => a.points.length >= 3).map((a) => ({ type: 'Feature' as const, properties: {}, geometry: { type: 'Polygon' as const, coordinates: [ring(a.points)] } })),
    });
    (m.getSource('draft') as GeoJSONSource).setData(
      draft.length > 1
        ? { type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: draft.map(([la, lo]) => [lo, la]) } }
        : empty(),
    );
    for (const k of markers) k.remove();
    markers = pins.map((p) => {
      const el = document.createElement('div');
      el.className = 'pinmark';
      el.innerHTML = '<span class="pdot"></span><span class="plab"></span>';
      (el.querySelector('.pdot') as HTMLElement).style.background = pinColor(p.kind);
      (el.querySelector('.plab') as HTMLElement).textContent = p.label; // text, never markup
      return new ml!.Marker({ element: el, anchor: 'left' }).setLngLat([p.lon, p.lat]).addTo(m);
    });
  });

  // ---------- find a place ----------
  let findTerm = $state('');
  let finds: PlaceHit[] = $state([]);
  let findNote = $state('');
  let findBusy = $state(false);
  let foundMarker: Marker | null = null;
  const ZOOM_FOR: Record<string, number> = { state: 6, county: 9, city: 11, zip: 13, point: 15 };

  async function findPlace() {
    const q = findTerm.trim();
    if (!q) return;
    findBusy = true;
    findNote = '';
    try {
      const r = await placeSearch(q);
      finds = r.places;
      if (!finds.length) {
        findNote = r.has_list
          ? 'Nothing matches. Try the town and state ("Denver, CO"), a ZIP code, or a latitude and longitude.'
          : 'Nothing matches. Without the places list on the drive only states and a latitude and longitude are found (see Settings, Maps, or run scripts\\get-maps.ps1 with -Places).';
      } else if (finds.length === 1) {
        goToPlace(finds[0]);
      }
    } catch (e) {
      findNote = 'The search could not run: ' + String(e);
    }
    findBusy = false;
  }
  function goToPlace(h: PlaceHit) {
    if (!map || !ml) return;
    finds = [];
    findTerm = h.name + (h.state ? ', ' + h.state : '');
    map.jumpTo({ center: [h.lon, h.lat], zoom: ZOOM_FOR[h.kind] ?? 12 });
    foundMarker?.remove();
    const el = document.createElement('div');
    el.className = 'pinmark';
    el.innerHTML = '<span class="pdot"></span><span class="plab"></span>';
    (el.querySelector('.pdot') as HTMLElement).style.background = '#5aa9e6';
    (el.querySelector('.plab') as HTMLElement).textContent = h.name;
    foundMarker = new ml.Marker({ element: el, anchor: 'left' }).setLngLat([h.lon, h.lat]).addTo(map);
  }
  function pinFound() {
    const m = foundMarker?.getLngLat();
    if (!m) return;
    const id = pins.reduce((a, x) => Math.max(a, x.id), 0) + 1;
    pins = [...pins, { id, label: findTerm || 'Pin ' + id, kind: 'pin', lat: Math.round(m.lat * 1e5) / 1e5, lon: Math.round(m.lng * 1e5) / 1e5 }];
  }

  async function refresh() {
    try {
      saved = await mapsList();
    } catch (e) {
      error = String(e);
    }
  }

  function parsePoint(lat: string, lon: string): LatLon | null {
    const a = Number(lat.trim());
    const b = Number(lon.trim());
    if (lat.trim() === '' || lon.trim() === '' || !Number.isFinite(a) || !Number.isFinite(b)) return null;
    if (a < -90 || a > 90 || b < -180 || b > 180) return null;
    return [a, b];
  }

  function addPin() {
    error = '';
    const p = parsePoint(pLat, pLon);
    if (!p) {
      error = 'Enter a latitude between -90 and 90 and a longitude between -180 and 180, as numbers (for example 39.4 and -80.1).';
      return;
    }
    const id = pins.reduce((m, x) => Math.max(m, x.id), 0) + 1;
    pins = [...pins, { id, label: pLabel.trim() || (pKind === 'rendezvous' ? 'Rendezvous ' + id : 'Pin ' + id), kind: pKind, lat: p[0], lon: p[1] }];
    pLabel = pLat = pLon = '';
  }
  function removePin(id: number) {
    pins = pins.filter((p) => p.id !== id);
    route = route.filter((r) => r !== id);
  }
  function toRoute(id: number) {
    route = [...route, id];
  }
  function addDraftPoint() {
    error = '';
    const p = parsePoint(aLat, aLon);
    if (!p) {
      error = 'Enter the corner as a latitude and a longitude.';
      return;
    }
    draft = [...draft, p];
    aLat = aLon = '';
  }
  function finishArea() {
    if (draft.length < 3) {
      error = 'An area needs at least three corners.';
      return;
    }
    nogo = [...nogo, { label: draftLabel.trim() || 'No-go area ' + (nogo.length + 1), points: draft }];
    draft = [];
    draftLabel = '';
    error = '';
  }

  let routePoints: LatLon[] = $derived(
    route.map((id) => pins.find((p) => p.id === id)).filter((p): p is Pin => !!p).map((p) => [p.lat, p.lon] as LatLon),
  );
  $effect(() => {
    const pts = routePoints;
    if (pts.length < 2) {
      stats = null;
      return;
    }
    routeStats(pts).then((s) => (stats = s)).catch(() => (stats = null));
  });

  function newMap() {
    current = null;
    name = notes = '';
    pins = [];
    route = [];
    nogo = [];
    draft = [];
    message = error = '';
    confirmDelete = false;
  }

  async function open(id: string) {
    try {
      const m = await mapLoad(id);
      const f = JSON.parse(m.json) as Partial<MapFile>;
      current = id;
      name = f.name ?? '';
      pins = f.pins ?? [];
      route = f.route ?? [];
      nogo = f.nogo ?? [];
      notes = m.notes;
      draft = [];
      message = error = '';
      confirmDelete = false;
      queueMicrotask(zoomToPoints);
    } catch (e) {
      error = 'Could not open that map: ' + String(e);
    }
  }

  async function save() {
    error = message = '';
    if (!name.trim()) {
      error = 'Give the map a name first.';
      return;
    }
    const file: MapFile = { version: 1, name: name.trim(), pins, route, nogo };
    try {
      current = await mapSave(current, name.trim(), JSON.stringify(file, null, 2), notes);
      message = 'Saved.';
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function exportGpx() {
    error = message = '';
    if (!current) {
      error = 'Save the map first, then export its route.';
      return;
    }
    try {
      const where = await mapExportGpx(current, name.trim() || 'Route', routePoints);
      message = 'Route written to ' + where;
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function remove() {
    if (!current) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    try {
      await mapDelete(current);
      newMap();
      await refresh();
      message = 'Map deleted.';
    } catch (e) {
      error = String(e);
    }
  }

  // ---------- plan view: your points, drawn to scale, with no map underneath ----------
  const W = 640;
  const H = 400;
  const PAD = 36;

  let view = $derived.by(() => {
    const all: LatLon[] = [...pins.map((p) => [p.lat, p.lon] as LatLon), ...nogo.flatMap((a) => a.points), ...draft];
    if (all.length === 0) return null;
    const lats = all.map((p) => p[0]);
    const lons = all.map((p) => p[1]);
    const minLat = Math.min(...lats), maxLat = Math.max(...lats);
    const minLon = Math.min(...lons), maxLon = Math.max(...lons);
    const midLat = (minLat + maxLat) / 2;
    const kx = Math.cos((midLat * Math.PI) / 180); // a degree of longitude is shorter away from the equator
    const spanX = Math.max((maxLon - minLon) * kx, 1e-4);
    const spanY = Math.max(maxLat - minLat, 1e-4);
    const scale = Math.min((W - 2 * PAD) / spanX, (H - 2 * PAD) / spanY);
    const offX = (W - spanX * scale) / 2;
    const offY = (H - spanY * scale) / 2;
    const x = (lon: number) => offX + (lon - minLon) * kx * scale;
    const y = (lat: number) => H - offY - (lat - minLat) * scale;
    // scale bar: pixels per kilometre, then a round length that fits
    const kmPerDeg = 111.195;
    const pxPerKm = scale / kmPerDeg;
    const options = [0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000];
    const km = [...options].reverse().find((k) => k * pxPerKm <= 150) ?? options[0];
    return { x, y, barPx: km * pxPerKm, barKm: km };
  });

  const pinColor = (k: string) => (k === 'rendezvous' ? '#6fbf73' : '#5aa9e6');
</script>

{#snippet pinList()}
  <ul class="plain">
    {#each pins as p}
      <li>
        <span class="dot" style="background:{pinColor(p.kind)}"></span>
        {p.label} <span class="muted">{p.lat.toFixed(4)}, {p.lon.toFixed(4)}</span>
        <button class="link" title="Go to this pin on the map" onclick={() => map?.jumpTo({ center: [p.lon, p.lat], zoom: Math.max(map.getZoom(), 12) })}>go to</button>
        <button class="link" title="Add this pin to the end of your route" onclick={() => toRoute(p.id)}>add to route</button>
        <button class="link" title="Delete this pin" onclick={() => removePin(p.id)}>remove</button>
      </li>
    {:else}
      <li class="muted">No pins yet. Choose Pins above, then click the map.</li>
    {/each}
  </ul>
{/snippet}

<svelte:window onkeydown={onKey} />

<section>
  <h1>Maps</h1>
  <p class="sub">Plan with your own points: places to meet, routes between them, and areas to avoid. Save the plan, print it, and export the route for a phone map app.</p>
</section>

{#if street}
  <div class="warn">
    <strong>Map: {street.name}</strong> ({street.mb} MB, from this drive).{#if packInfo} This pack covers {packInfo.replace(street.name + ': covers ', '')}{/if}{#if partial} It covers only part of the country, so the map stops at its edge. This build comes with only a small sample area. To get the whole United States, double-click scripts\Get-Maps.bat (it needs internet and tens of gigabytes of space, and takes hours).{/if} Roads, water, land and buildings are drawn{#if labelsOn}, with names{:else}; place names need the map fonts (run scripts\get-maps.ps1 -FontsOnly){/if}. Use Places on the map to mark hospitals, water, fuel and more. Positions you place are approximate, and routes are straight lines between your points: this app does not plan roads and does not yet steer routes around no-go areas.
  </div>
{:else}
  <div class="warn">
    <strong>No map tiles on this drive yet.</strong> Street and topo maps come from map packs (see doc 7), so for now this is a plan view of the points you enter, drawn to scale with nothing underneath. Positions are typed in by hand and are approximate. Routes are straight lines between your points; this app does not plan roads and does not yet steer routes around no-go areas.
  </div>
{/if}
{#if tileError}<div class="warn err" role="alert">{tileError}</div>{/if}

{#if error}<div class="warn err" role="alert">{error}</div>{/if}
{#if message}<div class="ok-note" role="status">{message}</div>{/if}

<div class="cols">
  <div class="panel side">
    <div class="label">My saved maps</div>
    <div class="list">
      {#each saved as m}
        <button class="item" class:on={m.id === current} onclick={() => open(m.id)}>
          {m.name}{#if m.has_route} <span class="muted">· route</span>{/if}
        </button>
      {:else}
        <span class="muted">Nothing saved yet.</span>
      {/each}
    </div>
    <button class="btn" onclick={newMap}>New map</button>
  </div>

  <div class="panel main">
    <div class="row">
      <input class="input grow" placeholder="Map name (for example: Home to the lake cabin)" bind:value={name} aria-label="Map name" />
      <button class="btn primary" onclick={save}>Save</button>
      <button class="btn" onclick={exportGpx} disabled={routePoints.length < 2}>Export route (GPX)</button>
      {#if current}<button class="btn danger" onclick={remove}>{confirmDelete ? 'Really delete?' : 'Delete'}</button>{/if}
    </div>

    {#if street}
      <div class="stage" class:full bind:this={stageEl}>
        <div class="tools">
          <div class="row">
            <span class="label">Click the map to add:</span>
            {#each [['off', 'Nothing', 'Clicking the map does nothing, so you can move it safely'], ['pin', 'Pins', 'Each click drops a pin'], ['rendezvous', 'Rendezvous points', 'Each click marks a place to meet'], ['corner', 'No-go corners', 'Each click adds a corner of an area to avoid']] as [k, t, tip]}
              <button class="btn" class:primary={addMode === k} title={tip} onclick={() => (addMode = k as typeof addMode)}>{t}</button>
            {/each}
            <button class="btn" title="Zoom so all your pins and areas fit on screen" onclick={zoomToPoints} disabled={pins.length === 0 && nogo.length === 0}>Zoom to my points</button>
            <button class="btn" class:primary={layersOpen} title="Choose which kinds of places are marked on the map: hospitals, water, fuel, shelter and more" onclick={() => (layersOpen = !layersOpen)}>Places on the map</button>
            <button class="btn" class:primary={cross} title="Show or hide the cross-hair and the coordinates of the middle of the map" onclick={() => (cross = !cross)}>Cross-hair</button>
            <button class="btn" title={full ? 'Go back to the normal page (or press Esc)' : 'Make the map fill the whole screen. Your pins and tools stay.'} onclick={toggleFull}>{full ? 'Exit full screen' : 'Full screen'}</button>
          </div>
          <div class="row find">
            <input class="input grow" type="search" placeholder="Find a place: ZIP code, town and state, park, lake, or latitude and longitude" aria-label="Find a place" bind:value={findTerm}
              onkeydown={(e) => { if (e.key === 'Enter') void findPlace(); }} />
            <button class="btn" title="Look this place up and move the map to it" onclick={findPlace} disabled={!findTerm.trim() || findBusy}>Find</button>
            <button class="btn" title="Keep the place you just found as a pin on your map" onclick={pinFound} disabled={!foundMarker}>Pin it</button>
          </div>
          {#if finds.length > 1}
            <ul class="finds" aria-label="Places found">
              {#each finds as h}
                <li><button onclick={() => goToPlace(h)}>{h.name}{h.state ? ', ' + h.state : ''} <span class="muted">{h.kind}</span></button></li>
              {/each}
            </ul>
          {/if}
          {#if findNote}<p class="muted fine">{findNote}</p>{/if}
          {#if addMode === 'corner'}
            <div class="row">
              <span class="muted">{draft.length} corner{draft.length === 1 ? '' : 's'} so far</span>
              <button class="btn" title="Close the area from the corners clicked so far" onclick={finishArea} disabled={draft.length < 3}>Finish area</button>
              {#if draft.length > 0}<button class="link" title="Throw away these corners and begin again" onclick={() => (draft = [])}>start over</button>{/if}
            </div>
          {/if}
        </div>
        <div class="mapbox">
          <div class="tilemap" bind:this={mapEl} role="application" aria-label="Map"></div>
          {#if cross}<div class="cross" aria-hidden="true"><span></span></div>{/if}
          {#if layersOpen}
            <aside class="layers" aria-label="Places on the map">
              <div class="row between"><span class="label">PLACES ON THE MAP</span><button class="link" title="Close this list" onclick={() => (layersOpen = false)}>close</button></div>
              <div class="row"><button class="link" title="Mark every kind of place" onclick={() => setAllGroups(true)}>all</button><button class="link" title="Mark none" onclick={() => setAllGroups(false)}>none</button></div>
              {#each POI_GROUPS as g}
                <label class="grp"><input type="checkbox" checked={poiOn[g.id]} onchange={(e) => setGroup(g.id, (e.currentTarget as HTMLInputElement).checked)} /><span class="swatch" style="background:{g.color}"></span>{g.label}</label>
              {/each}
              <p class="muted small">Places appear when you zoom in. {labelsOn ? 'Names show up close in.' : 'Names need the map fonts: run scripts\\get-maps.ps1 -FontsOnly.'} Click a marked place for its card.</p>
              <div class="label gap">FIND THE NEAREST</div>
              <div class="row">
                <select class="input grow" bind:value={nearGroup} aria-label="Kind of place to find">{#each POI_GROUPS as g}<option value={g.id}>{g.label}</option>{/each}</select>
                <button class="btn" title="List the closest places of this kind to the middle of the map" onclick={findNearest}>Find</button>
              </div>
              {#if nearNote}<p class="muted small">{nearNote}</p>{/if}
              {#each nearList as h}
                <button class="near" title="Move the map to this place" onclick={() => showPick(h)}><span>{h.name}</span><span class="muted">{kindName(h.kind)} · {dist(h.km)} {h.dir}</span></button>
              {/each}
            </aside>
          {/if}
          {#if picked}
            <div class="pick" role="dialog" aria-label="Marked place">
              <div><strong>{picked.name}</strong> <span class="muted small">{kindName(picked.kind)}{groupOf(picked.kind) ? ' · ' + groupOf(picked.kind)?.label : ''}</span></div>
              <div class="muted small">{picked.lat.toFixed(5)}, {picked.lon.toFixed(5)}</div>
              <div class="row">
                <button class="btn" title="Keep this place as a pin on your map" onclick={() => { if (picked) pinPick(picked); }}>Pin it</button>
                <button class="btn" title="Close this card" onclick={() => (picked = null)}>Close</button>
              </div>
            </div>
          {/if}
          <div class="hud" aria-live="off">
            {#if cursor}<div>CURSOR {cursor[0].toFixed(5)}, {cursor[1].toFixed(5)}</div>{/if}
            {#if cross && centre}<div>CENTRE {centre[0].toFixed(5)}, {centre[1].toFixed(5)}</div>{/if}
          </div>
          {#if full}
            <aside class="drawer" class:closed={!drawer}>
              <button class="link" title="Show or hide your list of pins and areas" onclick={() => (drawer = !drawer)}>{drawer ? 'Hide list ›' : '‹ My points'}</button>
              {#if drawer}
                <div class="label">My points</div>
                {@render pinList()}
                {#if route.length > 0}
                  <p class="muted small">Route: {route.map((id) => pins.find((p) => p.id === id)?.label ?? '?').join(' → ')}
                    <button class="link" title="Remove the route but keep the pins" onclick={() => (route = [])}>clear route</button></p>
                {/if}
                {#if stats}<p class="small"><strong>Route:</strong> {settings.values.mapUnits === 'metric' ? `${stats.km.toFixed(1)} km` : `${stats.miles.toFixed(1)} miles`}, straight line</p>{/if}
                <div class="label">No-go areas</div>
                <ul class="plain">
                  {#each nogo as a, i}
                    <li>{a.label} <button class="link" title="Remove this area" onclick={() => (nogo = nogo.filter((_, j) => j !== i))}>remove</button></li>
                  {:else}<li class="muted">None.</li>{/each}
                </ul>
                <div class="row">
                  <input class="input" placeholder="Map name" bind:value={name} aria-label="Map name" />
                  <button class="btn primary" title="Save this map and everything on it" onclick={save}>Save</button>
                </div>
                {#if message}<p class="small">{message}</p>{/if}
                {#if error}<p class="small errtxt">{error}</p>{/if}
              {/if}
            </aside>
          {/if}
        </div>
      </div>
    {:else}
      <svg viewBox="0 0 {W} {H}" role="img" aria-label="Plan view of your points" class="plan">
        <rect width={W} height={H} fill="#0f1110" />
        {#if view}
          {#each nogo as a}
            <polygon points={a.points.map((p) => view.x(p[1]) + ',' + view.y(p[0])).join(' ')} fill="#e5533d33" stroke="#e5533d" stroke-width="1.5" stroke-dasharray="5 4" />
          {/each}
          {#if draft.length > 0}
            <polyline points={draft.map((p) => view.x(p[1]) + ',' + view.y(p[0])).join(' ')} fill="none" stroke="#e5533d" stroke-width="1.5" stroke-dasharray="2 3" />
          {/if}
          {#if routePoints.length > 1}
            <polyline points={routePoints.map((p) => view.x(p[1]) + ',' + view.y(p[0])).join(' ')} fill="none" stroke="#ff9a1f" stroke-width="2.5" />
          {/if}
          {#each pins as p}
            <circle cx={view.x(p.lon)} cy={view.y(p.lat)} r="6" fill={pinColor(p.kind)} stroke="#0f1110" stroke-width="2" />
            <text x={view.x(p.lon) + 10} y={view.y(p.lat) + 4} fill="#ece7de" font-size="12">{p.label}</text>
          {/each}
          <line x1={PAD} y1={H - 14} x2={PAD + view.barPx} y2={H - 14} stroke="#b5ada0" stroke-width="2" />
          <text x={PAD + view.barPx + 8} y={H - 10} fill="#b5ada0" font-size="11">{settings.values.mapUnits === 'metric' ? `${view.barKm} km` : `${(view.barKm * 0.621371).toFixed(view.barKm < 1 ? 2 : 1)} mi`}</text>
          <text x={W - 24} y="22" fill="#b5ada0" font-size="12" text-anchor="middle">N ↑</text>
        {:else}
          <text x={W / 2} y={H / 2} fill="#9a9388" font-size="14" text-anchor="middle">Add a point below to start your plan.</text>
        {/if}
      </svg>
    {/if}

    {#if stats}
      <p class="stat"><strong>Route:</strong> {settings.values.mapUnits === 'metric' ? `${stats.km.toFixed(1)} km` : `${stats.miles.toFixed(1)} miles`} in a straight line through {routePoints.length} points. Real roads will be longer.</p>
    {/if}

    <div class="grid2">
      <div>
        <div class="label">Add a place</div>
        <div class="row">
          <input class="input" placeholder="Label" bind:value={pLabel} aria-label="Place label" />
          <select class="input" bind:value={pKind} aria-label="Kind of place">
            <option value="pin">Pin</option>
            <option value="rendezvous">Rendezvous point</option>
          </select>
        </div>
        <div class="row">
          <input class="input" placeholder="Latitude, e.g. 39.4" bind:value={pLat} aria-label="Latitude" inputmode="decimal" />
          <input class="input" placeholder="Longitude, e.g. -80.1" bind:value={pLon} aria-label="Longitude" inputmode="decimal" />
          <button class="btn" onclick={addPin}>Add</button>
        </div>
        {@render pinList()}
        {#if route.length > 0}
          <p class="muted">Route order: {route.map((id) => pins.find((p) => p.id === id)?.label ?? '?').join(' → ')}
            <button class="link" onclick={() => (route = [])}>clear route</button></p>
        {/if}
      </div>

      <div>
        <div class="label">No-go areas</div>
        <p class="muted small">Corners of a flooded, blocked or unsafe area, in order around its edge.</p>
        <div class="row">
          <input class="input" placeholder="Area label" bind:value={draftLabel} aria-label="No-go area label" />
        </div>
        <div class="row">
          <input class="input" placeholder="Latitude" bind:value={aLat} aria-label="Corner latitude" inputmode="decimal" />
          <input class="input" placeholder="Longitude" bind:value={aLon} aria-label="Corner longitude" inputmode="decimal" />
          <button class="btn" onclick={addDraftPoint}>Add corner</button>
        </div>
        <div class="row">
          <span class="muted">{draft.length} corner{draft.length === 1 ? '' : 's'} so far</span>
          <button class="btn" onclick={finishArea} disabled={draft.length < 3}>Finish area</button>
          {#if draft.length > 0}<button class="link" onclick={() => (draft = [])}>start over</button>{/if}
        </div>
        <ul class="plain">
          {#each nogo as a, i}
            <li>{a.label} <span class="muted">{a.points.length} corners</span>
              <button class="link" onclick={() => (nogo = nogo.filter((_, j) => j !== i))}>remove</button></li>
          {/each}
        </ul>
      </div>
    </div>

    <div class="label notes-label">Notes</div>
    <textarea class="input" bind:value={notes} placeholder="Written notes for this map: gates, water on the way, who meets where." aria-label="Notes"></textarea>
    <p class="muted small">Map data comes from © OpenStreetMap contributors. Place names: GeoNames (CC BY 4.0). Saved maps are plain files in the user/saved-maps folder.</p>
  </div>
</div>

<style>
  .cols { display: flex; flex-wrap: wrap; gap: 14px; align-items: flex-start; }
  .side { flex: 1 1 220px; max-width: 280px; display: flex; flex-direction: column; gap: 10px; }
  .main { flex: 4 1 480px; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .list { display: flex; flex-direction: column; gap: 4px; }
  .item { text-align: left; padding: 8px 10px; border-radius: 8px; border: 1px solid transparent; background: transparent; color: var(--text); }
  .item:hover { border-color: var(--line2); }
  .item.on { border-color: var(--blue-line); color: var(--blue); }
  .grow { flex: 1 1 220px; min-width: 0; }
  .plan { width: 100%; height: auto; border: 1px solid var(--line); border-radius: 10px; }
  .grid2 { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 18px; }
  .plain { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
  .dot { display: inline-block; width: 9px; height: 9px; border-radius: 50%; margin-right: 6px; }
  .link { background: none; border: 0; padding: 0 4px; color: var(--blue); text-decoration: underline; font-size: 12px; }
  .small { font-size: 12px; margin: 4px 0; }
  .stat { margin: 0; }
  .err { border-color: #7a2e22; background: #2a1511; color: #ffb4a6; }
  .ok-note { padding: 10px 12px; border-radius: 8px; background: #122117; border: 1px solid #2c5a3a; font-size: 13px; color: #b8e3c0; }
  .notes-label { margin-top: 4px; }
  input.input { min-width: 0; }
  .find { margin-top: 2px; }
  .finds { list-style: none; margin: 0; padding: 4px; border: 1px solid var(--line2); border-radius: 8px; background: var(--panel2); }
  .finds button { display: block; width: 100%; text-align: left; background: none; border: 0; padding: 6px 8px; border-radius: 6px; }
  .finds button:hover { background: var(--blue-soft); }
  .stage { display: flex; flex-direction: column; gap: 8px; }
  .stage.full { position: fixed; inset: 0; z-index: 2000; background: #0b0d0c; padding: 10px; }
  .tools { display: flex; flex-direction: column; gap: 8px; }
  .mapbox { position: relative; flex: 1; min-height: 0; }
  .stage.full .mapbox { display: flex; }
  .stage.full .tilemap { height: auto; flex: 1; }
  .cross { position: absolute; inset: 0; pointer-events: none; display: grid; place-items: center; }
  .cross span { width: 34px; height: 34px; position: relative; opacity: .85; }
  .cross span::before, .cross span::after { content: ''; position: absolute; background: #ff9a1f; }
  .cross span::before { left: 50%; top: 0; bottom: 0; width: 1px; }
  .cross span::after { top: 50%; left: 0; right: 0; height: 1px; }
  .hud { position: absolute; left: 10px; bottom: 28px; padding: 6px 9px; background: #0b0d0ccc; border: 1px solid #3a3f3a; border-left: 3px solid #ff9a1f; border-radius: 4px; font-family: var(--mono, monospace); font-size: 11px; letter-spacing: .06em; color: #cfd6cc; pointer-events: none; }
  .hud:empty { display: none; }
  .drawer { position: absolute; top: 10px; right: 10px; bottom: 28px; width: 290px; overflow: auto; padding: 10px; background: #0b0d0cee; border: 1px solid #3a3f3a; border-radius: 8px; display: flex; flex-direction: column; gap: 8px; z-index: 5; }
  .layers { position: absolute; top: 10px; left: 10px; bottom: 28px; width: 270px; overflow: auto; padding: 10px; background: #0b0d0cee; border: 1px solid #3a3f3a; border-radius: 8px; display: flex; flex-direction: column; gap: 6px; z-index: 5; }
  .grp { display: flex; align-items: center; gap: 8px; font-size: 13px; padding: 2px 0; }
  .swatch { width: 10px; height: 10px; border-radius: 50%; border: 1px solid #0b0d0c; flex: 0 0 auto; }
  .near { display: flex; flex-direction: column; text-align: left; gap: 1px; padding: 6px 8px; border: 1px solid var(--line); border-radius: 6px; background: transparent; font-size: 13px; }
  .near:hover { border-color: var(--blue-line); }
  .pick { position: absolute; left: 50%; transform: translateX(-50%); bottom: 34px; min-width: 240px; max-width: 360px; padding: 10px 12px; background: #0b0d0cf2; border: 1px solid #3a3f3a; border-left: 3px solid #ff9a1f; border-radius: 6px; display: flex; flex-direction: column; gap: 6px; z-index: 6; }
  .gap { margin-top: 8px; }
  .drawer.closed { width: auto; bottom: auto; }
  .errtxt { color: #ffb4a6; }
  .tilemap { width: 100%; height: clamp(520px, calc(100vh - 260px), 1200px); border: 1px solid var(--line); border-radius: 10px; overflow: hidden; background: #0f1110; }
  :global(.pinmark) { display: flex; align-items: center; gap: 6px; pointer-events: none; }
  :global(.pinmark .pdot) { width: 12px; height: 12px; border-radius: 50%; border: 2px solid #0f1110; display: inline-block; }
  :global(.pinmark .plab) { color: #ece7de; font-size: 12px; text-shadow: 0 0 3px #000, 0 0 3px #000; white-space: nowrap; }
</style>
