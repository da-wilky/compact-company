<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import EntryForm from '#lib/components/EntryForm.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { isoDay } from '#lib/dates.ts';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { clock, entryTitle, loadTimers, openTimer, timers } from '#lib/timers.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Entry = Schemas['TimeEntry'];
	const owner = $derived(session.view?.user?.role === 'owner');

	function monday(date: Date) {
		const day = new Date(date.getFullYear(), date.getMonth(), date.getDate());
		day.setDate(day.getDate() - ((day.getDay() + 6) % 7));
		return day;
	}
	const addDays = (date: Date, n: number) =>
		new Date(date.getFullYear(), date.getMonth(), date.getDate() + n);

	let week = $state(monday(new Date()));
	let person = $state('');
	let people = $state<Schemas['Assignee'][]>([]);
	let entries = $state<Entry[]>([]);
	let editing = $state<Entry>();
	let dialog = $state(false);
	let newDay = $state<string>();
	let confirming = $state(false);
	let error = $state<ApiError>();

	const days = $derived(Array.from({ length: 7 }, (_, i) => addDays(week, i)));
	const viewingSelf = $derived(!person || Number(person) === session.view?.user?.id);

	// ISO 8601 week number: the week with the year's first Thursday is week 1
	const weekNumber = $derived.by(() => {
		const thursday = addDays(week, 3);
		const first = new Date(thursday.getFullYear(), 0, 4);
		return 1 + Math.round((thursday.getTime() - monday(first).getTime()) / (7 * 86400000));
	});

	async function load() {
		const from = week.getTime() / 1000;
		const to = addDays(week, 7).getTime() / 1000;
		const user_id = person ? Number(person) : undefined;
		const result = await call(api.GET('/api/time', { params: { query: { from, to, user_id } } }));
		entries = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		[week, person, timers.list.length];
		load();
	});

	$effect(() => {
		if (owner) call(api.GET('/api/assignees')).then(({ data }) => (people = data ?? []));
	});

	const tracked = (e: Entry) => (e.ended_at ?? timers.now) - e.started_at;
	const charged = (e: Entry) => (e.billable ? (e.billable_secs ?? tracked(e)) : 0);
	const onDay = (day: Date) =>
		entries.filter((e) => isoDay(new Date(e.started_at * 1000)) === isoDay(day));
	const sum = (list: Entry[], f: (e: Entry) => number) => list.reduce((s, e) => s + f(e), 0);
	const hhmm = (secs: number) =>
		new Date(secs * 1000).toLocaleTimeString(i18n.locale, { hour: '2-digit', minute: '2-digit' });
	const dayLabel = (day: Date) =>
		day.toLocaleDateString(i18n.locale, { weekday: 'short', day: '2-digit', month: '2-digit' });
	const range = $derived(
		`${days[0].toLocaleDateString(i18n.locale, { day: 'numeric', month: 'short' })} – ${days[6].toLocaleDateString(i18n.locale, { day: 'numeric', month: 'short', year: 'numeric' })}`
	);
	const isToday = (day: Date) => isoDay(day) === isoDay(new Date());

	function add(day?: Date) {
		editing = undefined;
		newDay = day ? isoDay(day) : undefined;
		dialog = true;
	}

	function edit(entry: Entry) {
		editing = entry;
		dialog = true;
	}

	async function saved() {
		dialog = false;
		toast(t('app.saved'));
		await Promise.all([load(), loadTimers()]);
	}

	async function remove() {
		if (!editing) return;
		const id = editing.id;
		confirming = false;
		dialog = false;
		const result = await call(api.DELETE('/api/time/{id}', { params: { path: { id } } }));
		error = result.error;
		await Promise.all([load(), loadTimers()]);
	}
</script>

<div class="page-head">
	<div>
		<p class="muted">{t('time.week', { week: weekNumber })} · {range}</p>
		<h1>{t('time.title')}</h1>
	</div>
	<div class="row">
		<button class="icon" aria-label={t('time.previous_week')} onclick={() => (week = addDays(week, -7))}>‹</button>
		<button onclick={() => (week = monday(new Date()))}>{t('time.this_week')}</button>
		<button class="icon" aria-label={t('time.next_week')} onclick={() => (week = addDays(week, 7))}>›</button>
	</div>
</div>

<div class="stack">
	<div class="row">
		{#if owner && people.length > 1}
			<select class="person" aria-label={t('time.whose')} bind:value={person}>
				<option value="">{t('time.mine')}</option>
				{#each people.filter((p) => p.id !== session.view?.user?.id) as p (p.id)}
					<option value={String(p.id)}>{p.name}</option>
				{/each}
			</select>
		{/if}
		<div class="totals">
			<span><small>{t('time.total')}</small><strong>{clock(sum(entries, tracked))}</strong></span>
			<span><small>{t('time.billable_total')}</small><strong>{clock(sum(entries, charged))}</strong></span>
		</div>
		{#if viewingSelf}
			<div class="actions">
				<button onclick={() => add()}><Icon name="plus" size={18} />{t('time.new_entry')}</button>
				<button class="primary" onclick={() => openTimer()}><Icon name="play" size={16} />{t('time.start')}</button>
			</div>
		{/if}
	</div>

	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

	<div class="days">
		{#each days as day (day.getTime())}
			{@const list = onDay(day)}
			<section class="card day" class:today={isToday(day)}>
				<header>
					<h2>{dayLabel(day)}</h2>
					<span class="sum">{list.length ? clock(sum(list, tracked)) : ''}</span>
					{#if viewingSelf}
						<button class="ghost icon" aria-label={t('time.new_entry_on', { day: dayLabel(day) })} onclick={() => add(day)}>
							<Icon name="plus" size={16} />
						</button>
					{/if}
				</header>
				{#each list as entry (entry.id)}
					<button class="entry" onclick={() => edit(entry)}>
						<span class="when">{hhmm(entry.started_at)}–{entry.ended_at ? hhmm(entry.ended_at) : t('time.now')}</span>
						<span class="what">
							<span class="title">{entryTitle(entry)}</span>
							<small>
								{entry.customer_name} · {entry.project_name}{#if entry.activity_name}&nbsp;· {entry.activity_name}{/if}
								{#if !entry.billable}&nbsp;· {t('time.not_billable')}{/if}
							</small>
						</span>
						<span class="duration" class:running={!entry.ended_at}>{clock(tracked(entry))}</span>
					</button>
				{/each}
			</section>
		{/each}
	</div>
</div>

<Dialog bind:open={dialog} title={t(editing ? 'time.edit_entry' : 'time.new_entry')}>
	{#if dialog}
		<EntryForm mode="entry" entry={editing} day={newDay} onsaved={saved} />
	{/if}
	{#snippet actions()}
		{#if editing}
			<button class="ghost danger" onclick={() => (confirming = true)}>{t('app.delete')}</button>
		{/if}
	{/snippet}
</Dialog>

<Dialog bind:open={confirming} title={t('time.delete_title')}>
	<p>{t('time.delete_confirm')}</p>
	{#snippet actions()}
		<button onclick={() => (confirming = false)}>{t('app.cancel')}</button>
		<button class="primary" onclick={remove}>{t('app.delete')}</button>
	{/snippet}
</Dialog>

<style>
	.person {
		width: auto;
	}
	.totals {
		display: flex;
		gap: 1.5rem;
	}
	.totals span {
		display: flex;
		flex-direction: column;
	}
	.totals small {
		color: var(--muted);
		font-size: 0.75rem;
	}
	.totals strong {
		font: 600 1.25rem var(--font-display);
		font-variant-numeric: tabular-nums;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-left: auto;
	}
	.days {
		display: grid;
		gap: 0.75rem;
	}
	.day {
		padding: 0;
		overflow: hidden;
	}
	.day.today {
		border-color: var(--accent-soft-border);
	}
	.day header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.25rem 0.5rem 0.25rem 1rem;
		background: var(--surface-2);
	}
	.day h2 {
		font-size: 0.875rem;
	}
	.today h2 {
		color: var(--accent-text);
	}
	.sum {
		margin-left: auto;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
	}
	.entry {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr) auto;
		align-items: center;
		gap: 0.25rem 1rem;
		width: 100%;
		min-height: 3.25rem;
		padding: 0.5rem 1rem;
		border: 0;
		border-top: 1px solid var(--border);
		border-radius: 0;
		background: none;
		font-weight: 400;
		text-align: left;
		white-space: normal;
	}
	.entry:hover {
		background: var(--segment);
	}
	.when,
	.duration {
		font-variant-numeric: tabular-nums;
	}
	.when {
		color: var(--muted);
		font-size: 0.8125rem;
	}
	.what {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-weight: 500;
	}
	.what small {
		color: var(--muted);
		font-size: 0.75rem;
	}
	.duration {
		font-weight: 600;
	}
	.running {
		color: var(--accent-text);
	}
	@media (max-width: 599px) {
		.entry {
			grid-template-columns: minmax(0, 1fr) auto;
		}
		.when {
			grid-column: 1 / -1;
		}
	}
</style>
