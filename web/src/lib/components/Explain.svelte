<script lang="ts">
	import { explaining } from '#lib/help.svelte.ts';
	import { has, t } from '#lib/i18n.svelte.ts';
	import Dialog from './Dialog.svelte';

	let open = $state(false);
	$effect(() => {
		open = !!explaining.term;
	});
	$effect(() => {
		if (!open) explaining.term = undefined;
	});
</script>

<Dialog bind:open title={explaining.term ? t(`help.${explaining.term}.name`) : ''}>
	{#if explaining.term}
		{@const term = explaining.term}
		<div class="stack">
			<p>{t(`help.${term}.text`)}</p>
			{#if has(`help.${term}.law`)}<small class="muted">{t('help.law', { law: t(`help.${term}.law`) })}</small>{/if}
			<a href="/help#{term}" onclick={() => (open = false)}>{t('help.more')}</a>
		</div>
	{/if}
</Dialog>

<style>
	p {
		margin: 0;
		max-width: 36rem;
	}
</style>
