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
	import EntryForm from '#lib/components/EntryForm.svelte';
	import TimerList from '#lib/components/TimerList.svelte';
	import { load, session } from '#lib/session.svelte.ts';
	import { clock, entryTitle, loadTimers, timers } from '#lib/timers.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';
	import type { Schemas } from '#lib/api/index.ts';

	let { children } = $props();

	type Item = {
		href: string;
		label: string;
		icon: IconName;
		roles: Schemas['Role'][];
		group: 'main' | 'admin' | 'me';
	};
	const all: Schemas['Role'][] = ['owner', 'staff', 'accountant'];
	const items: Item[] = $derived([
		{ href: '/', label: t('nav.overview'), icon: 'overview', roles: all, group: 'main' },
		{ href: '/todos', label: t('todos.title'), icon: 'todos', roles: ['owner', 'staff'], group: 'main' },
		{ href: '/time', label: t('time.title'), icon: 'time', roles: ['owner', 'staff'], group: 'main' },
		{ href: '/customers', label: t('customers.title'), icon: 'customers', roles: all, group: 'main' },
		{ href: '/documents', label: t('documents.title'), icon: 'documents', roles: ['owner', 'accountant'], group: 'main' },
		{ href: '/receipts', label: t('receipts.title'), icon: 'receipts', roles: ['owner', 'accountant'], group: 'main' },
		{ href: '/bank', label: t('bank.title'), icon: 'bank', roles: ['owner', 'accountant'], group: 'main' },
		{ href: '/open-items', label: t('open_items.title'), icon: 'open_items', roles: ['owner', 'accountant'], group: 'main' },
		{ href: '/books', label: t('books.title'), icon: 'books', roles: ['owner', 'accountant'], group: 'main' },
		{ href: '/tax', label: t('tax.title'), icon: 'tax', roles: ['owner', 'accountant'], group: 'main' },
		{ href: '/activities', label: t('activities.title'), icon: 'activities', roles: ['owner'], group: 'admin' },
		{ href: '/statuses', label: t('statuses.title'), icon: 'statuses', roles: ['owner'], group: 'admin' },
		{ href: '/layouts', label: t('layouts.title'), icon: 'layouts', roles: ['owner'], group: 'admin' },
		{ href: '/snippets', label: t('snippets.title'), icon: 'comment', roles: ['owner'], group: 'admin' },
		{ href: '/suppliers', label: t('suppliers.title'), icon: 'suppliers', roles: ['owner', 'accountant'], group: 'admin' },
		{ href: '/business', label: t('term.business'), icon: 'business', roles: ['owner', 'accountant'], group: 'admin' },
		{ href: '/users', label: t('users.title'), icon: 'users', roles: ['owner'], group: 'admin' },
		{ href: '/audit', label: t('audit.title'), icon: 'audit', roles: ['owner', 'accountant'], group: 'admin' },
		{ href: '/account', label: t('account.title'), icon: 'account', roles: all, group: 'me' }
	]);

	const open = ['/setup', '/sign-in', '/invitation'];
	const user = $derived(session.view?.user);
	const visible = $derived(items.filter((item) => user && item.roles.includes(user.role)));
	const main = $derived(visible.filter((item) => item.group === 'main'));
	const admin = $derived(visible.filter((item) => item.group === 'admin'));
	// the phone bar shows the everyday pages; the rest live in the "more" sheet
	const bar = $derived(visible.filter((item) => item.group === 'main' && item.href !== '/').slice(0, 3));
	const initials = $derived(
		(user?.name ?? '')
			.split(/\s+/)
			.map((part) => part[0])
			.join('')
			.slice(0, 2)
			.toUpperCase()
	);
	let more = $state(false);
	let timerSheet = $state(false);
	const tracks = $derived(user?.role === 'owner' || user?.role === 'staff');
	const newest = $derived(timers.list[0]);

	$effect(() => {
		if (tracks) loadTimers();
		else timers.list = [];
	});

	async function timerStarted() {
		timers.dialog = false;
		toast(t('time.started'));
		await loadTimers();
	}

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

	const current = (href: string) =>
		page.url.pathname === href || (href !== '/' && page.url.pathname.startsWith(`${href}/`))
			? 'page'
			: undefined;

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
			<div class="bottom">
				{#if tracks}<TimerList />{/if}
			</div>
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

		<main class:with-timer={newest}>{@render children()}</main>

		{#if newest}
			<button class="timer-bar" onclick={() => (timerSheet = true)} aria-expanded={timerSheet}>
				<span class="timer-text">
					<span>{entryTitle(newest)}{#if timers.list.length > 1}&nbsp;· +{timers.list.length - 1}{/if}</span>
					<strong>{clock(Math.max(0, timers.now - newest.started_at), true)}</strong>
				</span>
				<span class="badge">{t('time.timers_running', { count: timers.list.length })}</span>
				<Icon name="chevron_up" size={18} />
			</button>
		{/if}

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

	<Dialog bind:open={timerSheet} title={t('time.running_timers')} sheet>
		<TimerList large />
	</Dialog>

	<Dialog bind:open={timers.dialog} title={t('time.start')}>
		{#if timers.dialog}
			<EntryForm mode="timer" preset={timers.preset} onsaved={timerStarted} />
		{/if}
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

	main.with-timer {
		padding-bottom: calc(var(--bottom-bar) + 6rem + env(safe-area-inset-bottom));
	}
	.timer-bar {
		position: fixed;
		left: 0.75rem;
		right: 0.75rem;
		bottom: calc(var(--bottom-bar) + 0.625rem + env(safe-area-inset-bottom));
		z-index: 5;
		display: flex;
		align-items: center;
		gap: 0.625rem;
		min-height: 3.5rem;
		padding: 0 0.875rem 0 1rem;
		border: 0;
		border-radius: var(--radius-lg);
		background: var(--accent);
		color: var(--accent-fg);
		box-shadow: var(--shadow-raised);
		text-align: left;
	}
	.timer-text {
		display: flex;
		flex-direction: column;
		min-width: 0;
		font-size: 0.75rem;
		font-weight: 400;
	}
	.timer-text span {
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.timer-text strong {
		font: 600 1.0625rem var(--font-display);
		font-variant-numeric: tabular-nums;
	}
	.timer-bar .badge {
		margin-left: auto;
		background: rgb(255 255 255 / 0.2);
		color: inherit;
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
		.sidebar .item {
			min-height: 2.25rem;
		}
		.bottom {
			margin-top: auto;
			padding: 1rem 0 0.75rem;
		}
		.timer-bar {
			display: none;
		}
		.me {
			display: flex;
			align-items: center;
			gap: 0.25rem;
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
