<script lang="ts">
	import { api, call, type Schemas } from '#lib/api/index.ts';
	import { dueLabel, formatDay } from '#lib/dates.ts';
	import { formatMoney } from '#lib/money.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { clock, timers } from '#lib/timers.svelte.ts';

	const user = $derived(session.view?.user);
	const works = $derived(user?.role === 'owner' || user?.role === 'staff');
	let todos = $state<Schemas['Todo'][]>([]);
	let week = $state<Schemas['TimeEntry'][]>([]);

	$effect(() => {
		if (!works || !user) return;
		call(api.GET('/api/todos', { params: { query: { assignee_id: user.id, done: false } } })).then(
			({ data }) =>
				(todos = (data ?? [])
					.toSorted((a, b) => (a.due_date ?? '9999').localeCompare(b.due_date ?? '9999'))
					.slice(0, 8))
		);
		const now = new Date();
		const monday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - ((now.getDay() + 6) % 7));
		const from = monday.getTime() / 1000;
		call(api.GET('/api/time', { params: { query: { from, to: from + 7 * 86400 } } })).then(
			({ data }) => (week = data ?? [])
		);
	});

	let overview = $state<Schemas['Overview']>();
	const money = $derived(user?.role === 'owner' || user?.role === 'accountant');
	$effect(() => {
		if (!user) return;
		timers.list.length;
		call(api.GET('/api/overview')).then(({ data }) => (overview = data));
	});

	const hours = $derived(week.reduce((sum, e) => sum + (e.ended_at ?? timers.now) - e.started_at, 0));
</script>

<div class="page-head">
	<div>
		<p class="muted">{t('nav.overview')}</p>
		<h1>{t('home.greeting', { name: user?.name ?? '' })}</h1>
	</div>
</div>

{#if works}
	<div class="grid">
		<section class="card stack">
			<h2>{t('home.my_todos')}</h2>
			{#if todos.length}
				<ul class="list">
					{#each todos as todo (todo.id)}
						{@const due = dueLabel(todo.due_date, todo.done)}
						<li>
							<div>
								<a href="/todos/{todo.id}">{todo.title}</a>
								<small class="muted">{todo.customer_name} · {todo.project_name}</small>
							</div>
							{#if due}<small class:overdue={due.overdue}>{due.text}</small>{/if}
						</li>
					{/each}
				</ul>
			{:else}
				<p class="muted">{t('home.no_todos')}</p>
			{/if}
			<a href="/todos">{t('home.all_todos')}</a>
		</section>
		<section class="card stack">
			<h2>{t('home.this_week')}</h2>
			<p class="hours">{clock(hours)}<small class="muted"> h</small></p>
			<p class="muted">{t('home.timers', { count: timers.list.length })}</p>
			<a href="/time">{t('home.to_time')}</a>
		</section>
	</div>
{/if}


{#if overview && (money || overview.running.length)}
	<div class="cards">
		{#if overview.running.length && user?.role === 'owner'}
			<section class="card stack">
				<h2>{t('home.running')}</h2>
				<ul class="list">
					{#each overview.running as timer (timer.id)}
						<li>
							<div>
								<strong>{timer.todo_title ?? timer.description ?? timer.project_name}</strong>
								<small class="muted">{timer.user_name} · {timer.customer_name} · {timer.project_name}</small>
							</div>
							<small class="muted">{clock(timers.now - timer.started_at)}</small>
						</li>
					{/each}
				</ul>
			</section>
		{/if}
		{#if money}
			<section class="card stack">
				<h2>{t('home.unbilled')}</h2>
				{#if overview.unbilled.length}
					<ul class="list">
						{#each overview.unbilled as project (project.project_id)}
							<li>
								<div>
									<strong>{project.project_name}</strong>
									<small class="muted">{project.customer_name} · {t('documents.unbilled_hours', { hours: (project.minutes / 60).toLocaleString(undefined, { maximumFractionDigits: 2 }) })}</small>
								</div>
								{#if project.amount}<span class="amount">{formatMoney(project.amount.minor, project.amount.currency)}</span>{/if}
							</li>
						{/each}
					</ul>
				{:else}
					<p class="muted">{t('home.nothing_unbilled')}</p>
				{/if}
			</section>
			<section class="card stack">
				<h2>{t('home.open_invoices')}</h2>
				{#if overview.open_invoices.length}
					<ul class="list">
						{#each overview.open_invoices as invoice (invoice.id)}
							<li>
								<div>
									<a href={`/documents/${invoice.id}`}>{invoice.number}</a>
									<small class="muted">{invoice.customer_name}{#if invoice.due_date}&nbsp;· {t('documents.due_date')} {formatDay(invoice.due_date, { dateStyle: 'medium' })}{/if}</small>
								</div>
								<span class="amount" class:overdue={invoice.overdue}>{formatMoney(invoice.due.minor, invoice.due.currency)}</span>
							</li>
						{/each}
					</ul>
				{:else}
					<p class="muted">{t('home.no_open_invoices')}</p>
				{/if}
				<a href="/documents">{t('documents.title')}</a>
			</section>
		{/if}
	</div>
{/if}

<style>
	.grid {
		display: grid;
		gap: 1rem;
		align-items: start;
	}
	@media (min-width: 900px) {
		.grid {
			grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
		}
	}
	li a {
		color: var(--text);
		font-weight: 500;
		text-decoration: none;
	}
	.overdue {
		color: var(--danger);
	}
	.cards {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 20rem), 1fr));
		gap: 1rem;
		margin-top: 1rem;
		align-items: start;
	}
	.amount {
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.hours {
		font: 700 2.5rem var(--font-display);
		font-variant-numeric: tabular-nums;
	}
</style>
