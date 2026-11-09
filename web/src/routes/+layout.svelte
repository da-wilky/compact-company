<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, call } from '#lib/api/index.ts';
	import { i18n, t, type Locale } from '#lib/i18n.svelte.ts';
	import { load, session } from '#lib/session.svelte.ts';

	let { children } = $props();

	const open = ['/setup', '/sign-in', '/invitation'];
	const role = $derived(session.view?.user?.role);

	load();

	$effect(() => {
		const view = session.view;
		if (!view) return;
		const path = page.url.pathname;
		if (view.setup_required && path !== '/setup') goto('/setup');
		else if (!view.setup_required && !view.user && !open.includes(path)) goto('/sign-in');
		else if (view.user && open.includes(path)) goto('/');
	});

	async function signOut() {
		await call(api.POST('/api/sign-out'));
		await load();
	}
</script>

<svelte:head>
	<title>compact-company</title>
</svelte:head>

<header>
	<a href="/">compact-company</a>
	<nav>
		{#if role === 'owner' || role === 'accountant'}
			<a href="/business">{t('term.business')}</a>
			<a href="/audit">{t('audit.title')}</a>
		{/if}
		{#if role === 'owner'}
			<a href="/users">{t('users.title')}</a>
		{/if}
	</nav>
	{#if session.view?.user}
		<a href="/account">{session.view.user.name}</a>
		<button onclick={signOut}>{t('sign_out')}</button>
	{/if}
	<select bind:value={i18n.locale} aria-label="Language">
		{#each ['de', 'en'] as Locale[] as locale (locale)}
			<option value={locale}>{locale.toUpperCase()}</option>
		{/each}
	</select>
</header>

<main>
	{#if session.view?.user || open.includes(page.url.pathname)}
		{@render children()}
	{/if}
</main>

<style>
	:global(body) {
		font-family: system-ui, sans-serif;
		margin: 0;
		color: #1c1c1c;
		background: #fafafa;
	}
	:global(input:not([type='checkbox']), textarea) {
		font: inherit;
		padding: 0.4rem;
		border: 1px solid #bbb;
		border-radius: 4px;
	}
	:global(.error) {
		color: #b00020;
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
