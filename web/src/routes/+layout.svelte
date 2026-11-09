<script lang="ts">
	import { i18n, t, type Locale } from '#lib/i18n.svelte.ts';

	let { children } = $props();
</script>

<svelte:head>
	<title>compact-company</title>
</svelte:head>

<header>
	<a href="/">compact-company</a>
	<nav>
		<a href="/business">{t('term.business')}</a>
	</nav>
	<select bind:value={i18n.locale} aria-label="Language">
		{#each ['de', 'en'] as Locale[] as locale (locale)}
			<option value={locale}>{locale.toUpperCase()}</option>
		{/each}
	</select>
</header>

<main>
	{@render children()}
</main>

<style>
	:global(body) {
		font-family: system-ui, sans-serif;
		margin: 0;
		color: #1c1c1c;
		background: #fafafa;
	}
	header {
		display: flex;
		gap: 1.5rem;
		align-items: center;
		padding: 0.75rem 1rem;
		border-bottom: 1px solid #ddd;
		background: #fff;
	}
	nav {
		display: flex;
		gap: 1rem;
		flex: 1;
	}
	main {
		max-width: 40rem;
		padding: 1rem;
	}
</style>
