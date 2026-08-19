<script lang="ts">
  // Stands in for the MapLibre surface under jsdom, which has no WebGL. One
  // button, one fixed coordinate: enough to prove the dialog stages a pick.
  let { onpick = (_latitude: number, _longitude: number) => {} } = $props();
</script>

<button type="button" data-testid="map-pick" onclick={() => onpick(25.8, -80.1)}>pick</button>
