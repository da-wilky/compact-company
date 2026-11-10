<script lang="ts">
	import '#lib/theme.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, call } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Icon, { type IconName } from '#lib/components/Icon.svelte';
	import Logo from '#lib/components/Logo.svelte';
	import Preferences from '#lib/components/Preferences.svelte';
	import Toaster from '#lib/components/Toaster.svelte';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { applyTheme } from '#lib/preferences.svelte.ts';
	import { load, session } from '#lib/session.svelte.ts';
	import type { Schemas } from '#lib/api/index.ts';

	let { children } = $props();

	type Item = { href: string; label: string; icon: IconName; roles: Schemas['Role'][] };
	const all: Schemas['Role'][] = ['owner', 'staff', 'accountant'];
	const items: Item[] = $derived([
		{ href: '/', label: t('nav.overview'), icon: 'overview', roles: all },
		{ href: '/business', label: t('term.business'), icon: 'business', roles: ['owner', 'accountant'] },
		{ href: '/users', label: t('users.title'), icon: 'users', roles: ['owner'] },
		{ href: '/audit', label: t('audit.title'), icon: 'audit', roles: ['owner', 'accountant'] },
		{ href: '/account', label: t('account.title'), icon: 'account', roles: all }
	]);

	const open = ['/setup', '/sign-in', '/invitation'];
	const user = $derived(session.view?.user);
	const visible = $derived(items.filter((item) => user && item.roles.includes(user.role)));
	const main = $derived(visible.filter((item) => item.href === '/'));
	const admin = $derived(visible.filter((item) => item.href !== '/' && item.href !== '/account'));
	const bar = $derived(visible.slice(0, 3));
	const initials = $derived(
		(user?.name ?? '')
			.split(/\s+/)
			.map((part) => part[0])
			.join('')
			.slice(0, 2)
			.toUpperCase()
	);
	let more = $state(false);

	applyTheme();
	load();

	$effect(() => {
		document.documentElement.lang = i18n.locale;
	});

	$effect(() => {
		const view = session.view;
		if (!view) return;
		const path = page.url.pathname;
		if (view.setup_required && path !== '/setup') goto('/setup');
		else if (!view.setup_required && !view.user && !open.includes(path)) goto('/sign-in');
		else if (view.user && open.includes(path)) goto('/');
	});

	$effect(() => {
		page.url.pathname;
		more = false;
	});

	const current = (href: string) => (page.url.pathname === href ? 'page' : undefined);

	async function signOut() {
		await call(api.POST('/api/sign-out'));
		await load();
	}
</script>

<svelte:head>
	<title>compact-company</title>
</svelte:head>

{#if user}
	<div class="shell">
		<nav class="sidebar" aria-label={t('nav.main')}>
			<a class="brand" href="/"><Logo /></a>
			{#each main as item (item.href)}
				<a class="item" href={item.href} aria-current={current(item.href)}>
					<Icon name={item.icon} />{item.label}
				</a>
			{/each}
			{#if admin.length}
				<p class="group">{t('nav.admin')}</p>
				{#each admin as item (item.href)}
					<a class="item" href={item.href} aria-current={current(item.href)}>
						<Icon name={item.icon} />{item.label}
					</a>
				{/each}
			{/if}
			<div class="me">
				<a href="/account" aria-current={current('/account')}>
					<span class="avatar" aria-hidden="true">{initials}</span>
					<span class="who">
						<span>{user.name}</span>
						<small>{t(`term.${user.role}`)}</small>
					</span>
				</a>
				<button class="ghost icon" onclick={signOut} aria-label={t('sign_out')} title={t('sign_out')}>
					<Icon name="sign_out" />
				</button>
			</div>
		</nav>

		<main>{@render children()}</main>

		<nav class="bar" aria-label={t('nav.main')}>
			{#each bar as item (item.href)}
				<a href={item.href} aria-current={current(item.href)}>
					<Icon name={item.icon} size={22} />{item.label}
				</a>
			{/each}
			<button onclick={() => (more = true)} aria-expanded={more}>
				<Icon name="more" size={22} />{t('nav.more')}
			</button>
		</nav>
	</div>

	<Dialog bind:open={more} title={t('nav.more')} sheet>
		<div class="stack">
			<nav class="sheet-nav" aria-label={t('nav.more')}>
				{#each visible as item (item.href)}
					<a class="item" href={item.href} aria-current={current(item.href)}>
						<Icon name={item.icon} />{item.label}
					</a>
				{/each}
			</nav>
			<Preferences />
			<button onclick={signOut}><Icon name="sign_out" />{t('sign_out')}</button>
		</div>
	</Dialog>
{:else if open.includes(page.url.pathname) && session.view}
	<div class="auth">
		<main class="card stack">
			<Logo size={36} />
			{@render children()}
		</main>
		<Preferences />
	</div>
{/if}

<Toaster />

<style>
	.shell {
		min-height: 100dvh;
	}
	main {
		max-width: 72rem;
		padding: 1.5rem 1rem calc(var(--bottom-bar) + 2rem + env(safe-area-inset-bottom));
	}

	.sidebar {
		display: none;
	}
	.item {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		min-height: var(--control);
		padding: 0 0.75rem;
		border-radius: var(--radius);
		color: var(--muted);
		text-decoration: none;
	}
	.item:hover {
		background: var(--segment);
		color: var(--text);
	}
	.item[aria-current='page'] {
		background: var(--accent-soft);
		color: var(--accent-text);
		font-weight: 600;
	}

	.bar {
		position: fixed;
		inset: auto 0 0;
		z-index: 5;
		display: grid;
		grid-auto-columns: minmax(0, 1fr);
		grid-auto-flow: column;
		height: calc(var(--bottom-bar) + env(safe-area-inset-bottom));
		padding-bottom: env(safe-area-inset-bottom);
		border-top: 1px solid var(--border);
		background: var(--surface);
	}
	.bar a,
	.bar button {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 0.25rem;
		min-height: 0;
		padding: 0;
		border: 0;
		border-radius: 0;
		background: none;
		color: var(--muted);
		font-size: 0.6875rem;
		font-weight: 500;
		text-decoration: none;
		white-space: normal;
		text-align: center;
	}
	.bar [aria-current='page'] {
		color: var(--accent-text);
		font-weight: 600;
	}

	.sheet-nav {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	@media (min-width: 960px) {
		.shell {
			display: grid;
			grid-template-columns: var(--sidebar) minmax(0, 1fr);
		}
		main {
			padding: 2rem 2.5rem;
		}
		.bar {
			display: none;
		}
		.sidebar {
			position: sticky;
			top: 0;
			height: 100dvh;
			display: flex;
			flex-direction: column;
			gap: 0.25rem;
			padding: 1.5rem 0.875rem 0.875rem;
			border-right: 1px solid var(--border);
			background: var(--surface);
			overflow-y: auto;
		}
		.brand {
			padding: 0 0.625rem 1.25rem;
			text-decoration: none;
		}
		.group {
			margin: 1rem 0.75rem 0.25rem;
			color: var(--muted);
			font-size: 0.75rem;
			font-weight: 600;
			letter-spacing: 0.04em;
			text-transform: uppercase;
		}
		.me {
			display: flex;
			align-items: center;
			gap: 0.25rem;
			margin-top: auto;
			padding-top: 0.75rem;
			border-top: 1px solid var(--border);
		}
		.me a {
			flex: 1;
			min-width: 0;
			display: flex;
			align-items: center;
			gap: 0.625rem;
			padding: 0.375rem;
			border-radius: var(--radius);
			color: var(--text);
			text-decoration: none;
		}
		.me a:hover,
		.me a[aria-current='page'] {
			background: var(--segment);
		}
	}
	.avatar {
		display: grid;
		place-items: center;
		flex: none;
		width: 2rem;
		height: 2rem;
		border-radius: 50%;
		background: var(--accent-soft);
		color: var(--accent-text);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.who {
		display: flex;
		flex-direction: column;
		min-width: 0;
		font-weight: 500;
	}
	.who > span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.who small {
		color: var(--muted);
		font-size: 0.75rem;
		font-weight: 400;
	}

	.auth {
		min-height: 100dvh;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 1.25rem;
		padding: 2rem 1rem;
	}
	.auth main {
		width: 100%;
		max-width: 26rem;
		padding: 2rem 1.5rem;
		gap: 1.25rem;
	}
</style>
