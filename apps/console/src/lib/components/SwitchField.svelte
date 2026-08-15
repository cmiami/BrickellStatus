<script lang="ts">
  let {
    checked,
    label,
    description,
    disabled = false,
    onchange
  }: {
    checked: boolean;
    label: string;
    description?: string;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  } = $props();
</script>

<button
  type="button"
  class="switch-field"
  role="switch"
  aria-checked={checked}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="switch-track" aria-hidden="true"><span></span></span>
  <span class="switch-copy">
    <strong>{label}</strong>
    {#if description}<small>{description}</small>{/if}
  </span>
</button>

<style>
  .switch-field {
    display: grid;
    width: 100%;
    grid-template-columns: 42px minmax(0, 1fr);
    align-items: center;
    gap: 13px;
    color: var(--graphite);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 2px;
    padding: 8px;
    text-align: left;
    cursor: pointer;
  }

  .switch-field:hover:not(:disabled) {
    background: var(--paper);
    border-color: var(--rule);
  }

  .switch-track {
    position: relative;
    width: 40px;
    height: 22px;
    background: var(--steel);
    border: 1px solid var(--graphite);
    border-radius: 2px;
  }

  .switch-track span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    background: var(--white);
    border: 1px solid var(--graphite);
    transition:
      transform 160ms ease-out,
      background-color 160ms ease-out;
  }

  .switch-field[aria-checked='true'] .switch-track {
    background: var(--marine);
  }

  .switch-field[aria-checked='true'] .switch-track span {
    background: var(--amber);
    transform: translateX(18px);
  }

  .switch-copy {
    display: grid;
    gap: 3px;
  }

  .switch-copy strong {
    font-family: var(--font-instrument);
    font-size: var(--type-body-small);
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0.025em;
    text-transform: uppercase;
  }

  .switch-copy small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }
</style>
