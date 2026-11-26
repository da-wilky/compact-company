<script lang="ts">
	import type { Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import Icon from './Icon.svelte';
	import { session } from '#lib/session.svelte.ts';

	let { steps }: { steps: Schemas['GettingStarted'] } = $props();

	const links = {
		business: '/business',
		layout: '/layouts',
		books: '/books',
		customer: '/customers',
		invoice: '/documents',
		bank: '/bank',
		receipt: '/receipts',
		process_documentation: '/process-documentation'
	} as const;
	// paying through the private account under simple books, there is no statement to import
	const order = $derived(
		(Object.keys(links) as (keyof typeof links)[]).filter(
			(step) => step !== 'bank' || !session.view?.simple_books || session.view.business_bank
		)
	);
	const done = $derived(order.filter((step) => steps[step]).length);

	const key = 'getting-started-hidden';
	let hidden = $state(read());
	function read() {
		try {
			return localStorage.getItem(key) === '1';
		} catch {
			return false;
		}
	}
	function hide() {
		hidden = true;
		try {
			localStorage.setItem(key, '1');
		} catch {
			// no storage in private windows
		}
	}
</script>

{#if !hidden && done < order.length}
	<section class="card stack">
		<div class="head">
			<div>
				<h2>{t('help.start.title')}</h2>
				<small class="muted">{t('help.start.progress', { done, total: order.length })}</small>
			</div>
			<button class="ghost" onclick={hide}>{t('help.start.hide')}</button>
		</div>
		<progress max={order.length} value={done}></progress>
		<ol>
			{#each order as step (step)}
				<li class:done={steps[step]}>
					<span class="mark" aria-hidden="true">{#if steps[step]}<Icon name="check" size={14} />{/if}</span>
					{#if steps[step]}
						<span>{t(`help.start.${step}`)}</span>
					{:else}
						<a href={links[step]}>{t(`help.start.${step}`)}</a>
					{/if}
				</li>
			{/each}
		</ol>
		<a href="/help">{t('help.more')}</a>
	</section>
{/if}

<style>
	section {
		margin-bottom: 1rem;
	}
	.head {
		display: flex;
		justify-content: space-between;
		align-items: start;
		gap: 1rem;
	}
	progress {
		width: 100%;
		height: 0.375rem;
		accent-color: var(--accent);
	}
	ol {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 20rem), 1fr));
		gap: 0.5rem 1.5rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	li {
		display: flex;
		align-items: center;
		gap: 0.625rem;
	}
	.done {
		color: var(--muted);
		text-decoration: line-through;
	}
	.mark {
		display: grid;
		place-items: center;
		flex: none;
		width: 1.25rem;
		height: 1.25rem;
		border: 1px solid var(--border);
		border-radius: 50%;
	}
	.done .mark {
		border-color: var(--accent);
		background: var(--accent);
		color: var(--accent-fg);
	}
</style>
