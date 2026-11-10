<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import TodoForm from '#lib/components/TodoForm.svelte';
	import { dueLabel } from '#lib/dates.ts';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';
	import { loadWork } from '#lib/work.svelte.ts';

	const id = $derived(Number(page.params.id));
	let detail = $state<Schemas['TodoDetail']>();
	let statuses = $state<Schemas['Status'][]>([]);
	let projects = $state<Schemas['Project'][]>([]);
	let assignees = $state<Schemas['Assignee'][]>([]);
	let editing = $state(false);
	let deleting = $state(false);
	let comment = $state('');
	let error = $state<ApiError>();

	loadWork().then((work) => ({ statuses, projects, assignees } = work));

	async function load() {
		const result = await call(api.GET('/api/todos/{id}', { params: { path: { id } } }));
		detail = result.data;
		error = result.error;
	}

	$effect(() => {
		id;
		load();
	});

	async function setStatus(statusId: number) {
		const result = await call(
			api.POST('/api/todos/{id}/move', { params: { path: { id } }, body: { status_id: statusId } })
		);
		error = result.error;
		await load();
	}

	async function addComment(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(
			api.POST('/api/todos/{id}/comments', { params: { path: { id } }, body: { body: comment } })
		);
		error = result.error;
		if (result.data) {
			comment = '';
			await load();
		}
	}

	async function remove() {
		deleting = false;
		const result = await call(api.DELETE('/api/todos/{id}', { params: { path: { id } } }));
		error = result.error;
		if (!result.error) {
			toast(t('todos.deleted'));
			goto('/todos');
		}
	}

	const when = (secs: number) =>
		new Date(secs * 1000).toLocaleString(i18n.locale, { dateStyle: 'medium', timeStyle: 'short' });
</script>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if detail}
	{@const todo = detail.todo}
	{@const due = dueLabel(todo.due_date, todo.done)}
	<div class="page-head">
		<div>
			<p class="muted"><a href="/todos">{t('todos.title')}</a> · {todo.customer_name} · {todo.project_name}</p>
			<h1>{todo.title}</h1>
		</div>
		<div class="row">
			<button onclick={() => (editing = true)}>{t('app.edit')}</button>
			<button class="ghost danger" onclick={() => (deleting = true)}>{t('app.delete')}</button>
		</div>
	</div>

	<div class="layout">
		<div class="stack">
			<section class="card">
				{#if todo.description}
					<p class="pre">{todo.description}</p>
				{:else}
					<p class="muted">{t('todos.no_description')}</p>
				{/if}
			</section>

			<section class="card stack">
				<h2>{t('todos.comments')}</h2>
				{#each detail.comments as c (c.id)}
					<article class="comment">
						<header><strong>{c.user_name}</strong> <small class="muted">{when(c.created_at)}</small></header>
						<p class="pre">{c.body}</p>
					</article>
				{:else}
					<p class="muted">{t('todos.no_comments')}</p>
				{/each}
				<form class="stack" onsubmit={addComment}>
					<Field label={t('todos.add_comment')} error={error?.fields?.body}>
						<textarea rows="3" bind:value={comment}></textarea>
					</Field>
					<div><button class="primary">{t('todos.comment')}</button></div>
				</form>
			</section>
		</div>

		<aside class="card">
			<dl>
				<dt>{t('term.status')}</dt>
				<dd>
					<select aria-label={t('term.status')} value={todo.status_id} onchange={(e) => setStatus(Number(e.currentTarget.value))}>
						{#each statuses as status (status.id)}<option value={status.id}>{status.name}</option>{/each}
					</select>
				</dd>
				<dt>{t('term.priority')}</dt>
				<dd>{t(`priority.${todo.priority}`)}</dd>
				<dt>{t('todos.due_date')}</dt>
				<dd class:overdue={due?.overdue}>{due?.text ?? '–'}</dd>
				<dt>{t('todos.assignee')}</dt>
				<dd>{todo.assignee_name ?? t('todos.nobody')}</dd>
				<dt>{t('term.customer')}</dt>
				<dd><a href="/customers/{todo.customer_id}">{todo.customer_name}</a></dd>
			</dl>
		</aside>
	</div>

	<Dialog bind:open={editing} title={t('todos.edit')}>
		{#if editing}
			<TodoForm
				{todo}
				{statuses}
				{projects}
				{assignees}
				onsaved={async () => {
					editing = false;
					toast(t('app.saved'));
					await load();
				}}
			/>
		{/if}
	</Dialog>

	<Dialog bind:open={deleting} title={t('todos.delete_title')}>
		<p>{t('todos.delete_confirm', { title: todo.title })}</p>
		{#snippet actions()}
			<button onclick={() => (deleting = false)}>{t('app.cancel')}</button>
			<button class="primary" onclick={remove}>{t('app.delete')}</button>
		{/snippet}
	</Dialog>
{/if}

<style>
	.muted a {
		color: inherit;
	}
	.layout {
		display: grid;
		gap: 1rem;
		align-items: start;
	}
	@media (min-width: 900px) {
		.layout {
			grid-template-columns: minmax(0, 1fr) 18rem;
		}
	}
	.pre {
		white-space: pre-line;
	}
	.comment {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		padding-bottom: 0.75rem;
		border-bottom: 1px solid var(--border);
	}
	dl {
		display: grid;
		gap: 0.375rem;
		margin: 0;
	}
	dt {
		color: var(--muted);
		font-size: 0.75rem;
	}
	dd {
		margin: 0 0 0.5rem;
	}
	.overdue {
		color: var(--danger);
	}
</style>
