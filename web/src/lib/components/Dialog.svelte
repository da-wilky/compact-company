<script lang="ts">
	import type { Snippet } from 'svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import Icon from './Icon.svelte';

	let {
		open = $bindable(false),
		title,
		sheet = false,
		children,
		actions
	}: {
		open?: boolean;
		title: string;
		sheet?: boolean;
		children: Snippet;
		actions?: Snippet;
	} = $props();

	const id = $props.id();
	let dialog: HTMLDialogElement;

	$effect(() => {
		if (open && !dialog.open) dialog.showModal();
		else if (!open && dialog.open) dialog.close();
	});
</script>

<dialog
	bind:this={dialog}
	class:sheet
	onclose={() => (open = false)}
	onclick={(e) => e.target === dialog && (open = false)}
	aria-labelledby={id}
>
	<header>
		<h2 {id}>{title}</h2>
		<button class="ghost icon" onclick={() => (open = false)} aria-label={t('app.close')}>
			<Icon name="close" />
		</button>
	</header>
	<div class="body">{@render children()}</div>
	{#if actions}<footer>{@render actions()}</footer>{/if}
</dialog>

<style>
	dialog {
		width: min(28rem, calc(100vw - 2rem));
		padding: 0;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--surface);
		color: var(--text);
		box-shadow: var(--shadow-raised);
	}
	dialog::backdrop {
		background: var(--overlay);
	}
	header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.75rem 0.75rem 0 1.25rem;
	}
	header button {
		margin-left: auto;
	}
	.body {
		padding: 0.75rem 1.25rem 1.25rem;
	}
	footer {
		display: flex;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: 0.5rem;
		padding: 0 1.25rem 1.25rem;
	}
	.sheet {
		margin: auto 0 0;
		width: 100%;
		max-width: 100%;
		border-radius: 18px 18px 0 0;
		padding-bottom: env(safe-area-inset-bottom);
	}
</style>
