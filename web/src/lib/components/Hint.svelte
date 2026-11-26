<script lang="ts">
	import { explaining, type Term } from '#lib/help.svelte.ts';
	import { t } from '#lib/i18n.svelte.ts';

	let { term }: { term: Term } = $props();
</script>

<button
	type="button"
	class="hint"
	aria-label={t('help.explain', { term: t(`help.${term}.name`) })}
	title={t('help.explain', { term: t(`help.${term}.name`) })}
	onclick={(event) => {
		event.preventDefault();
		explaining.term = term;
	}}>?</button
>

<style>
	.hint {
		display: inline-grid;
		place-items: center;
		width: 1.25rem;
		height: 1.25rem;
		min-height: 0;
		margin-inline-start: 0.375rem;
		padding: 0;
		border: 1px solid var(--border);
		border-radius: 50%;
		background: var(--surface);
		color: var(--muted);
		font: 600 0.75rem/1 var(--font);
		vertical-align: middle;
	}
	.hint:hover {
		border-color: var(--accent);
		color: var(--accent-text);
	}
</style>
