<script lang="ts">
	import { untrack } from 'svelte';
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { isoDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { clock } from '#lib/timers.svelte.ts';
	import Field from './Field.svelte';

	let {
		mode,
		entry,
		preset = {},
		day,
		onsaved
	}: {
		/** `timer` starts one now; `entry` records or edits a span of time. */
		mode: 'timer' | 'entry';
		entry?: Schemas['TimeEntry'];
		preset?: { project_id?: number; todo_id?: number };
		day?: string;
		onsaved: (entry: Schemas['TimeEntry']) => void;
	} = $props();

	// the form edits a copy; later changes to the props do not reset what the user typed
	const initial = untrack(() => entry);
	const start = initial ? new Date(initial.started_at * 1000) : undefined;
	const end = initial?.ended_at ? new Date(initial.ended_at * 1000) : undefined;
	const time = (date: Date) =>
		`${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;

	let projects = $state<Schemas['Project'][]>([]);
	let activities = $state<Schemas['Activity'][]>([]);
	let todos = $state<Schemas['Todo'][]>([]);
	let project = $state(String(initial?.project_id ?? untrack(() => preset.project_id) ?? ''));
	let todo = $state(String(initial?.todo_id ?? untrack(() => preset.todo_id) ?? ''));
	let activity = $state(String(initial?.activity_id ?? ''));
	let description = $state(initial?.description ?? '');
	let date = $state(start ? isoDay(start) : (untrack(() => day) ?? isoDay(new Date())));
	let from = $state(start ? time(start) : '');
	let to = $state(end ? time(end) : '');
	let billable = $state(initial?.billable ?? true);
	let billableText = $state(initial?.billable_secs == null ? '' : clock(initial.billable_secs));
	let error = $state<ApiError>();
	const running = initial != null && initial.ended_at == null;

	const chosen = $derived(projects.find((p) => String(p.id) === project));
	const customers = $derived(
		[...new Set(projects.map((p) => p.customer_name))].map((name) => ({
			name,
			projects: projects.filter((p) => p.customer_name === name)
		}))
	);

	Promise.all([
		call(api.GET('/api/projects', { params: { query: { archived: false } } })),
		call(api.GET('/api/activities', { params: { query: { archived: false } } }))
	]).then(([p, a]) => {
		const open = (p.data ?? []).filter((x) => !x.closed || x.id === initial?.project_id);
		projects = open;
		activities = a.data ?? [];
		if (!project && open.length) project = String(open[0].id);
		if (!initial && chosen) billable = chosen.billable;
	});

	$effect(() => {
		const id = Number(project);
		if (!id) return;
		call(api.GET('/api/todos', { params: { query: { project_id: id, done: false } } })).then(
			({ data }) => {
				todos = data ?? [];
				const current = initial?.todo_id;
				if (todo && !todos.some((x) => String(x.id) === todo) && Number(todo) !== current) todo = '';
			}
		);
	});

	function chooseProject(value: string) {
		project = value;
		todo = '';
		if (!initial) billable = projects.find((p) => String(p.id) === value)?.billable ?? true;
	}

	/** Seconds since the epoch for a local day and time; `to` before `from` means past midnight. */
	function instant(day: string, hhmm: string, after?: number) {
		const [y, m, d] = day.split('-').map(Number);
		const [h, min] = hhmm.split(':').map(Number);
		let secs = new Date(y, m - 1, d, h, min).getTime() / 1000;
		if (after != null && secs < after) secs += 24 * 3600;
		return secs;
	}

	function parseDuration(text: string): number | null {
		if (!text.trim()) return null;
		const match = text.trim().match(/^(\d+)(?::(\d{1,2}))?$/);
		if (!match || Number(match[2] ?? 0) > 59) return NaN;
		return Number(match[1]) * 3600 + Number(match[2] ?? 0) * 60;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const base = {
			project_id: Number(project),
			todo_id: todo ? Number(todo) : null,
			activity_id: activity ? Number(activity) : null,
			description
		};
		if (mode === 'timer') {
			const result = await call(api.POST('/api/timers', { body: base }));
			error = result.error;
			if (result.data) onsaved(result.data);
			return;
		}
		if (!from || (!to && !running)) {
			error = { ...clientError('error.invalid_input'), fields: from ? { ended_at: 'required' } : { started_at: 'required' } };
			return;
		}
		const billable_secs = parseDuration(billableText);
		if (Number.isNaN(billable_secs)) {
			error = { ...clientError('error.invalid_input'), fields: { billable_secs: 'invalid_format' } };
			return;
		}
		const started_at = instant(date, from);
		const body: Schemas['EntryInput'] = {
			...base,
			started_at,
			ended_at: to ? instant(date, to, started_at) : null,
			billable,
			billable_secs
		};
		const result = initial
			? await call(api.PUT('/api/time/{id}', { params: { path: { id: initial.id } }, body }))
			: await call(api.POST('/api/time', { body }));
		error = result.error;
		if (result.data) onsaved(result.data);
	}
</script>

<form class="grid" onsubmit={save}>
	<div class="wide">
		<Field label={t('term.project')} error={error?.fields?.project_id}>
			<select value={project} onchange={(e) => chooseProject(e.currentTarget.value)}>
				{#each customers as customer (customer.name)}
					<optgroup label={customer.name}>
						{#each customer.projects as p (p.id)}<option value={String(p.id)}>{p.name}</option>{/each}
					</optgroup>
				{/each}
			</select>
		</Field>
	</div>
	<Field label={t('term.todo')} error={error?.fields?.todo_id}>
		<select bind:value={todo}>
			<option value="">{t('time.no_todo')}</option>
			{#if initial?.todo_id && !todos.some((x) => x.id === initial.todo_id)}
				<option value={String(initial.todo_id)}>{initial.todo_title}</option>
			{/if}
			{#each todos as x (x.id)}<option value={String(x.id)}>{x.title}</option>{/each}
		</select>
	</Field>
	<Field label={t('term.activity')} error={error?.fields?.activity_id}>
		<select bind:value={activity}>
			<option value="">{t('time.no_activity')}</option>
			{#if initial?.activity_id && !activities.some((a) => a.id === initial.activity_id)}
				<option value={String(initial.activity_id)}>{initial.activity_name}</option>
			{/if}
			{#each activities as a (a.id)}<option value={String(a.id)}>{a.name}</option>{/each}
		</select>
	</Field>
	<div class="wide">
		<Field label={t('time.description')} error={error?.fields?.description}>
			<input bind:value={description} />
		</Field>
	</div>
	{#if mode === 'entry'}
		<Field label={t('time.date')}>
			<input type="date" bind:value={date} required />
		</Field>
		<div class="span">
			<Field label={t('time.from')} error={error?.fields?.started_at}>
				<input type="time" bind:value={from} />
			</Field>
			<Field label={t('time.to')} error={error?.fields?.ended_at} note={running ? t('time.running') : undefined}>
				<input type="time" bind:value={to} />
			</Field>
		</div>
		<Field label={t('time.billable')} inline>
			<input type="checkbox" bind:checked={billable} />
		</Field>
		<Field label={t('time.billable_duration')} error={error?.fields?.billable_secs} note={t('time.billable_hint')}>
			<input bind:value={billableText} placeholder="h:mm" inputmode="numeric" />
		</Field>
	{/if}
	{#if error}<p class="alert wide" role="alert">{t(error.message)}</p>{/if}
	<div class="wide">
		<button class="primary" disabled={!project}>{t(mode === 'timer' ? 'time.start' : 'app.save')}</button>
	</div>
</form>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
		gap: 1rem 1.25rem;
	}
	.wide {
		grid-column: 1 / -1;
	}
	.span {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.75rem;
	}
</style>
