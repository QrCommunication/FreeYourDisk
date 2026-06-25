<script lang="ts">
  let {
    checked = false,
    label = "",
    onchange,
  }: {
    checked?: boolean;
    label?: string;
    onchange: (value: boolean) => void;
  } = $props();
</script>

<button
  role="switch"
  aria-checked={checked}
  aria-label={label}
  onclick={() => onchange(!checked)}
  class="relative h-6 w-11 shrink-0 cursor-pointer rounded-full transition-colors"
  class:bg-accent={checked}
  class:bg-line={!checked}
>
  <span
    class="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform"
    class:translate-x-5={checked}
  ></span>
</button>
