<script lang="ts">
  let {
    values,
    rising = true,
    label
  }: {
    values: number[];
    rising?: boolean;
    label: string;
  } = $props();

  const WIDTH = 100;
  const HEIGHT = 28;

  // Scaled to its own range rather than to zero. A stock that moved from 511 to
  // 514 is a flat line against a zero axis and a legible one against its own
  // day, and the day is the question being asked.
  const path = $derived.by(() => {
    if (values.length < 2) return '';
    const low = Math.min(...values);
    const high = Math.max(...values);
    const span = high - low || 1;
    return values
      .map((value, index) => {
        const x = (index / (values.length - 1)) * WIDTH;
        const y = HEIGHT - ((value - low) / span) * HEIGHT;
        return `${index === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(' ');
  });
</script>

{#if path}
  <svg
    class="sparkline"
    class:falling={!rising}
    viewBox="0 0 {WIDTH} {HEIGHT}"
    preserveAspectRatio="none"
    role="img"
    aria-label={label}
  >
    <path d={path} fill="none" stroke="currentColor" stroke-width="1.75" vector-effect="non-scaling-stroke" />
  </svg>
{/if}

<style>
  .sparkline {
    width: 100%;
    height: 30px;
    color: var(--success);
    overflow: visible;
  }

  .sparkline.falling {
    color: var(--danger);
  }
</style>
