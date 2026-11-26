<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Schemas } from '#lib/api/index.ts';
	import type { Term } from '#lib/help.svelte.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import Hint from './Hint.svelte';

	let {
		label,
		error,
		note,
		suffix,
		hint,
		inline = false,
		children
	}: {
		label: string;
		error?: Schemas['FieldError'];
		note?: string;
		/** Shown right after the input, such as a currency. */
		suffix?: string;
		hint?: Term;
		inline?: boolean;
		children: Snippet;
	} = $props();
</script>

<label class:invalid={error} class:inline>
	<span class="label">{label}{#if hint}<Hint term={hint} />{/if}</span>
	{#if suffix}
		<span class="group">{@render children()}<span class="suffix">{suffix}</span></span>
	{:else}
		{@render children()}
	{/if}
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
	.group {
		display: flex;
	}
	.group :global(input) {
		min-width: 0;
		border-start-end-radius: 0;
		border-end-end-radius: 0;
	}
	.suffix {
		display: flex;
		align-items: center;
		padding: 0 0.625rem;
		border: 1px solid var(--border);
		border-inline-start: none;
		border-start-end-radius: var(--radius);
		border-end-end-radius: var(--radius);
		background: var(--surface-2);
		color: var(--muted);
		font-size: 0.875rem;
	}
</style>
