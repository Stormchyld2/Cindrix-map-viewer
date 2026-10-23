// The kinds of places the map can mark, from the "pois" layer of the Protomaps/OpenStreetMap tiles.
// Each group has its own colour; the groups can be switched on and off on the Maps screen.
// The list of kinds in the tiles is not complete (some raw OpenStreetMap values pass through), so a group
// simply shows the kinds it names that are present.

export type PoiGroup = { id: string; label: string; color: string; kinds: string[]; on: boolean };

export const POI_GROUPS: PoiGroup[] = [
  { id: 'emergency', label: 'Emergency and health', color: '#e5533d', on: true, kinds: ['hospital', 'clinic', 'doctors', 'dentist', 'pharmacy', 'fire_station', 'police', 'emergency_phone', 'life_ring'] },
  { id: 'water', label: 'Drinking water', color: '#5aa9e6', on: true, kinds: ['drinking_water', 'water_point', 'watering_place'] },
  { id: 'fuel', label: 'Fuel and charging', color: '#ffc247', on: true, kinds: ['fuel', 'charging_station'] },
  { id: 'food', label: 'Food and supplies', color: '#6fbf73', on: false, kinds: ['supermarket', 'grocery', 'convenience', 'bakery', 'butcher', 'greengrocer', 'hardware', 'garden_centre'] },
  { id: 'shelter', label: 'Shelter and lodging', color: '#b58be8', on: false, kinds: ['shelter', 'hotel', 'hostel', 'guest_house', 'bed_and_breakfast', 'alpine_hut', 'wilderness_hut', 'chalet'] },
  { id: 'camp', label: 'Camping and outdoors', color: '#4fd1c5', on: false, kinds: ['camp_site', 'picnic_site', 'firepit', 'ranger_station', 'viewpoint', 'peak', 'hunting_stand'] },
  { id: 'transport', label: 'Transport', color: '#ff9a1f', on: false, kinds: ['aerodrome', 'airfield', 'station', 'bus_stop', 'marina', 'slipway', 'car_repair', 'car_parts', 'car_rental', 'taxi'] },
  { id: 'services', label: 'Services and civic', color: '#cfd6cc', on: false, kinds: ['townhall', 'post_office', 'library', 'school', 'college', 'university', 'atm', 'bank', 'bureau_de_change', 'toilets', 'sanitary_dump_station', 'shower', 'recycling'] },
  { id: 'military', label: 'Military and border', color: '#9a9388', on: false, kinds: ['military', 'naval_base', 'customs'] },
];

/** "fire_station" becomes "Fire station". */
export function kindName(kind: string): string {
  const s = kind.replace(/_/g, ' ').trim();
  return s ? s[0].toUpperCase() + s.slice(1) : 'Place';
}

/** Which group a kind belongs to, if any. */
export function groupOf(kind: string): PoiGroup | undefined {
  return POI_GROUPS.find((g) => g.kinds.includes(kind));
}

/** Distance in kilometres between two points (great circle). */
export function distanceKm(a: [number, number], b: [number, number]): number {
  const R = 6371.0088;
  const rad = (d: number) => (d * Math.PI) / 180;
  const dLat = rad(b[0] - a[0]);
  const dLon = rad(b[1] - a[1]);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(rad(a[0])) * Math.cos(rad(b[0])) * Math.sin(dLon / 2) ** 2;
  return 2 * R * Math.asin(Math.min(1, Math.sqrt(h)));
}

/** Compass direction (N, NE, E ...) from a to b, both [latitude, longitude]. */
export function compass(a: [number, number], b: [number, number]): string {
  const rad = (d: number) => (d * Math.PI) / 180;
  const y = Math.sin(rad(b[1] - a[1])) * Math.cos(rad(b[0]));
  const x = Math.cos(rad(a[0])) * Math.sin(rad(b[0])) - Math.sin(rad(a[0])) * Math.cos(rad(b[0])) * Math.cos(rad(b[1] - a[1]));
  const deg = ((Math.atan2(y, x) * 180) / Math.PI + 360) % 360;
  return ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'][Math.round(deg / 45) % 8];
}
