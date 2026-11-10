<script lang="ts">
	import { untrack } from 'svelte';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import Field from './Field.svelte';

	let {
		todo,
		statuses,
		projects,
		assignees,
		onsaved
	}: {
		todo?: Schemas['Todo'];
		statuses: Schemas['Status'][];
		projects: Schemas['Project'][];
		assignees: Schemas['Assignee'][];
		onsaved: (todo: Schemas['Todo']) => void;
	} = $props();

	// the form edits a copy; later changes to the props do not reset what the user typed
	const initial = untrack(() => todo);
	let project_id = $state(initial?.project_id ?? untrack(() => projects[0]?.id));
	let title = $state(initial?.title ?? '');
	let description = $state(initial?.description ?? '');
	let status_id = $state(initial?.status_id ?? untrack(() => statuses[0]?.id));
	let priority = $state<Schemas['Priority']>(initial?.priority ?? 'normal');
	let due_date = $state(initial?.due_date ?? '');
	let assignee = $state(initial?.assignee_id == null ? '' : String(initial.assignee_id));
	let error = $state<ApiError>();

	const customers = $derived(
		[...new Set(projects.map((p) => p.customer_name))].map((name) => ({
			name,
			projects: projects.filter((p) => p.customer_name === name)
		}))
	);
	// a todo keeps its project after that closes, so the closed one stays selectable
	const keptProject = $derived(
		initial && !projects.some((p) => p.id === initial.project_id) ? initial : undefined
	);

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const body: Schemas['TodoInput'] = {
			project_id: project_id ?? 0,
			title,
			description,
			status_id,
			priority,
			due_date: due_date || null,
			assignee_id: assignee ? Number(assignee) : null
		};
		const result = initial
			? await call(api.PUT('/api/todos/{id}', { params: { path: { id: initial.id } }, body }))
			: await call(api.POST('/api/todos', { body }));
		error = result.error;
		if (result.data) onsaved(result.data);
	}
</script>

<form class="grid" onsubmit={save}>
	<div class="wide">
		<Field label={t('todos.title_field')} error={error?.fields?.title}>
			<input bind:value={title} />
		</Field>
	</div>
	<div class="wide">
		<Field label={t('term.project')} error={error?.fields?.project_id}>
			<select bind:value={project_id}>
				{#if keptProject}
					<option value={keptProject.project_id}>{keptProject.customer_name} · {keptProject.project_name}</option>
				{/if}
				{#each customers as customer (customer.name)}
					<optgroup label={customer.name}>
						{#each customer.projects as project (project.id)}
							<option value={project.id}>{project.name}</option>
						{/each}
					</optgroup>
				{/each}
			</select>
		</Field>
	</div>
	<Field label={t('term.status')}>
		<select bind:value={status_id}>
			{#each statuses as status (status.id)}<option value={status.id}>{status.name}</option>{/each}
		</select>
	</Field>
	<Field label={t('term.priority')}>
		<select bind:value={priority}>
			{#each ['low', 'normal', 'high'] as const as p (p)}<option value={p}>{t(`priority.${p}`)}</option>{/each}
		</select>
	</Field>
	<Field label={t('todos.due_date')} error={error?.fields?.due_date}>
		<input type="date" bind:value={due_date} />
	</Field>
	<Field label={t('todos.assignee')} error={error?.fields?.assignee_id}>
		<select bind:value={assignee}>
			<option value="">{t('todos.nobody')}</option>
			{#if initial?.assignee_id != null && !assignees.some((a) => a.id === initial.assignee_id)}
				<option value={String(initial.assignee_id)}>{initial.assignee_name}</option>
			{/if}
			{#each assignees as person (person.id)}<option value={String(person.id)}>{person.name}</option>{/each}
		</select>
	</Field>
	<div class="wide">
		<Field label={t('todos.description')} error={error?.fields?.description}>
			<textarea rows="4" bind:value={description}></textarea>
		</Field>
	</div>
	{#if error}<p class="alert wide" role="alert">{t(error.message)}</p>{/if}
	<div class="wide"><button class="primary">{t('app.save')}</button></div>
</form>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
		gap: 1rem 1.25rem;
	}
	.wide {
		grid-column: 1 / -1;
	}
</style>
