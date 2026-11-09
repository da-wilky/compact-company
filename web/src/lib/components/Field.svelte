<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';

	let {
		label,
		error,
		note,
		children
	}: { label: string; error?: Schemas['FieldError']; note?: string; children: Snippet } = $props();
</script>

<label class:invalid={error}>
	<span>{label}</span>
	{@render children()}
	{#if error}<small class="error">{t(`field.${error}`)}</small>{/if}
	{#if note}<small>{note}</small>{/if}
</label>

<style>
	label {
		display: grid;
		gap: 0.25rem;
		margin-bottom: 1rem;
	}
	small {
		color: #666;
	}
	.error {
		color: #b00020;
	}
	.invalid :global(:is(input, textarea)) {
		border-color: #b00020;
	}
</style>
