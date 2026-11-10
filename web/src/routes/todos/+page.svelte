<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Board from '#lib/components/Board.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import TodoForm from '#lib/components/TodoForm.svelte';
	import { dueLabel, today } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { loadWork } from '#lib/work.svelte.ts';

	type View = 'list' | 'board';
	const stored = (() => {
		try {
			return localStorage.getItem('todos.view');
		} catch {
			return null;
		}
	})();
	let view = $state<View>(stored === 'list' ? 'list' : 'board');
	let todos = $state<Schemas['Todo'][]>([]);
	let statuses = $state<Schemas['Status'][]>([]);
	let projects = $state<Schemas['Project'][]>([]);
	let assignees = $state<Schemas['Assignee'][]>([]);
	let project = $state('');
	let mine = $state(false);
	let overdue = $state(false);
	let showDone = $state(false);
	let creating = $state(false);
	let error = $state<ApiError>();

	loadWork().then((work) => ({ statuses, projects, assignees } = work));

	async function load() {
		const query = {
			project_id: project ? Number(project) : undefined,
			assignee_id: mine ? session.view?.user?.id : undefined,
			due_before: overdue ? today() : undefined,
			done: overdue || (view === 'list' && !showDone) ? false : undefined
		};
		const result = await call(api.GET('/api/todos', { params: { query } }));
		todos = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		[project, mine, overdue, showDone, view];
		load();
	});

	function setView(next: View) {
		view = next;
		try {
			localStorage.setItem('todos.view', next);
		} catch {
			// the choice then lasts for this visit
		}
	}

	async function move(todo: Schemas['Todo'], statusId: number, beforeId: number | null) {
		const others = todos.filter((t) => t.id !== todo.id);
		const at = beforeId == null ? -1 : others.findIndex((t) => t.id === beforeId);
		const moved = { ...todo, status_id: statusId };
		if (at < 0) {
			const last = others.findLastIndex((t) => t.status_id === statusId);
			others.splice(last + 1 || others.length, 0, moved);
		} else others.splice(at, 0, moved);
		todos = others;
		const result = await call(
			api.POST('/api/todos/{id}/move', {
				params: { path: { id: todo.id } },
				body: { status_id: statusId, before_id: beforeId }
			})
		);
		error = result.error;
		await load();
	}

	const statusName = (id: number) => statuses.find((s) => s.id === id)?.name ?? '';
</script>

<div class="page-head">
	<div>
		<p class="muted">{t('todos.count', { count: todos.length })}</p>
		<h1>{t('todos.title')}</h1>
	</div>
	<Segmented
		label={t('todos.view')}
		options={[
			{ value: 'list', label: t('todos.list') },
			{ value: 'board', label: t('todos.board') }
		]}
		value={view}
		onchange={setView}
	/>
	<button class="primary" onclick={() => (creating = true)} disabled={!projects.length}>
		<Icon name="plus" size={18} />{t('todos.new')}
	</button>
</div>

<div class="stack">
	<div class="row filters">
		<select aria-label={t('term.project')} bind:value={project}>
			<option value="">{t('todos.all_projects')}</option>
			{#each projects as p (p.id)}<option value={String(p.id)}>{p.customer_name} · {p.name}</option>{/each}
		</select>
		<button class="chip" aria-pressed={mine} onclick={() => (mine = !mine)}>{t('todos.mine')}</button>
		<button class="chip" aria-pressed={overdue} onclick={() => (overdue = !overdue)}>{t('todos.overdue')}</button>
		{#if view === 'list'}
			<button class="chip" aria-pressed={showDone} onclick={() => (showDone = !showDone)}>{t('todos.show_done')}</button>
		{/if}
	</div>

	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if !projects.length}<p class="card empty">{t('todos.no_projects')}</p>{/if}

	{#if view === 'board'}
		<Board {statuses} {todos} onmove={move} />
	{:else if todos.length}
		<div class="table-wrap stacked">
			<table>
				<thead>
					<tr>
						<th>{t('todos.title_field')}</th>
						<th>{t('term.project')}</th>
						<th>{t('term.status')}</th>
						<th>{t('term.priority')}</th>
						<th>{t('todos.due_date')}</th>
						<th>{t('todos.assignee')}</th>
					</tr>
				</thead>
				<tbody>
					{#each todos as todo (todo.id)}
						{@const due = dueLabel(todo.due_date, todo.done)}
						<tr class:done={todo.done}>
							<td><a href="/todos/{todo.id}">{todo.title}</a></td>
							<td data-label={t('term.project')}>{todo.customer_name} · {todo.project_name}</td>
							<td data-label={t('term.status')}><span class="badge">{statusName(todo.status_id)}</span></td>
							<td data-label={t('term.priority')}>{t(`priority.${todo.priority}`)}</td>
							<td data-label={t('todos.due_date')} class:overdue={due?.overdue}>{due?.text ?? '–'}</td>
							<td data-label={t('todos.assignee')}>{todo.assignee_name ?? '–'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}
		<p class="card empty">{t('todos.none')}</p>
	{/if}
</div>

<Dialog bind:open={creating} title={t('todos.new')}>
	{#if creating}
		<TodoForm {statuses} {projects} {assignees} onsaved={(todo) => goto(`/todos/${todo.id}`)} />
	{/if}
</Dialog>

<style>
	.filters select {
		width: auto;
		max-width: 100%;
		min-height: 36px;
		padding-block: 0.25rem;
	}
	.chip {
		min-height: 36px;
		padding: 0 0.875rem;
		border-radius: 999px;
		font-weight: 400;
	}
	.chip[aria-pressed='true'] {
		border-color: var(--accent-soft-border);
		background: var(--accent-soft);
		color: var(--accent-text);
	}
	td a {
		color: var(--text);
		font-weight: 500;
	}
	.done td {
		color: var(--muted);
	}
	.overdue {
		color: var(--danger);
	}
</style>
