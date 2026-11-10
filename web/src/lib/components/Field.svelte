<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';

	let {
		label,
		error,
		note,
		inline = false,
		children
	}: {
		label: string;
		error?: Schemas['FieldError'];
		note?: string;
		inline?: boolean;
		children: Snippet;
	} = $props();
</script>

<label class:invalid={error} class:inline>
	<span class="label">{label}</span>
	{@render children()}
	{#if error}<small class="error">{t(`field.${error}`)}</small>{/if}
	{#if note}<small>{note}</small>{/if}
</label>

<style>
	label {
		display: grid;
		gap: 0.375rem;
		align-content: start;
	}
	.inline {
		grid-template-columns: auto 1fr;
		align-items: center;
		column-gap: 0.625rem;
	}
	.inline .label {
		order: 1;
		font-weight: 400;
	}
	.inline small {
		order: 2;
		grid-column: 1 / -1;
	}
	.label {
		font-weight: 500;
	}
	small {
		color: var(--muted);
		font-size: 0.75rem;
	}
	.error {
		color: var(--danger);
	}
	.invalid :global(:is(input, textarea, select)) {
		border-color: var(--danger);
	}
</style>
