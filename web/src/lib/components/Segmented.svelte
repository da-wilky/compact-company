<script lang="ts" generics="T extends string">
	let {
		label,
		options,
		value,
		onchange
	}: {
		label: string;
		options: { value: T; label: string }[];
		value: T;
		onchange: (value: T) => void;
	} = $props();
</script>

<div role="radiogroup" aria-label={label}>
	{#each options as option (option.value)}
		<button
			type="button"
			role="radio"
			aria-checked={option.value === value}
			onclick={() => onchange(option.value)}>{option.label}</button
		>
	{/each}
</div>

<style>
	div {
		display: inline-flex;
		padding: 3px;
		border-radius: 9px;
		background: var(--segment);
	}
	button {
		min-height: calc(var(--control) - 6px);
		padding: 0 0.75rem;
		border: 0;
		border-radius: 7px;
		background: transparent;
		color: var(--muted);
		font-weight: 400;
	}
	button[aria-checked='true'] {
		background: var(--segment-active);
		color: var(--text);
		font-weight: 500;
		box-shadow: var(--shadow);
	}
</style>
