<script lang="ts">
	import { api, call, type Schemas } from '#lib/api/index.ts';
	import { dueLabel } from '#lib/dates.ts';
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
{:else}
	<section class="card stack">
		<p>{t('home.accountant')}</p>
		<a href="/customers">{t('customers.title')}</a>
	</section>
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
	.hours {
		font: 700 2.5rem var(--font-display);
		font-variant-numeric: tabular-nums;
	}
</style>
