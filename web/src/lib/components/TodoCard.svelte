<script lang="ts">
	import type { Schemas } from '#lib/api/index.ts';
	import { dueLabel } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { openTimer, stopTimer, timers } from '#lib/timers.svelte.ts';
	import Icon from './Icon.svelte';

	let {
		todo,
		statuses,
		lifted = false,
		onstatus
	}: {
		todo: Schemas['Todo'];
		statuses: Schemas['Status'][];
		lifted?: boolean;
		onstatus?: (statusId: number) => void;
	} = $props();

	const due = $derived(dueLabel(todo.due_date, todo.done));
	const running = $derived(timers.list.find((e) => e.todo_id === todo.id));
	const initials = $derived(
		(todo.assignee_name ?? '')
			.split(/\s+/)
			.map((part) => part[0])
			.join('')
			.slice(0, 2)
			.toUpperCase()
	);
</script>

<article class="card-item" class:lifted data-todo={todo.id}>
	<a class="title" href="/todos/{todo.id}" draggable="false">{todo.title}</a>
	<span class="meta">{todo.project_name} · {todo.customer_name}</span>
	<div class="row">
		{#if todo.priority !== 'normal'}
			<span class="badge priority-{todo.priority}">{t(`priority.${todo.priority}`)}</span>
		{/if}
		{#if due}<span class="due" class:overdue={due.overdue}>{due.text}</span>{/if}
		{#if todo.comments}
			<span class="comments" title={t('todos.comments')}><Icon name="comment" size={14} />{todo.comments}</span>
		{/if}
		{#if todo.assignee_name}
			<span class="avatar" title={todo.assignee_name}>{initials}</span>
		{/if}
		{#if running}
			<button class="timer on" aria-label={t('time.stop')} title={t('time.stop')} onclick={() => stopTimer(running.id)}>
				<Icon name="stop" size={12} />
			</button>
		{:else if !todo.done}
			<button
				class="timer"
				aria-label={t('time.start_on', { title: todo.title })}
				title={t('time.start')}
				onclick={() => openTimer({ project_id: todo.project_id, todo_id: todo.id })}
			>
				<Icon name="play" size={12} />
			</button>
		{/if}
	</div>
	{#if onstatus}
		<select
			class="status"
			aria-label={t('term.status')}
			value={todo.status_id}
			onchange={(e) => onstatus(Number(e.currentTarget.value))}
		>
			{#each statuses as status (status.id)}<option value={status.id}>{status.name}</option>{/each}
		</select>
	{/if}
</article>

<style>
	.card-item {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		padding: 0.75rem 0.875rem;
		border: 1px solid var(--border);
		border-radius: 10px;
		background: var(--surface);
		box-shadow: var(--shadow);
		user-select: none;
		-webkit-user-select: none;
		-webkit-touch-callout: none;
		cursor: grab;
	}
	.lifted {
		opacity: 0.35;
	}
	.title {
		color: var(--text);
		font-weight: 500;
		line-height: 1.35;
		text-decoration: none;
	}
	.title:hover {
		text-decoration: underline;
	}
	.meta {
		color: var(--muted);
		font-size: 0.75rem;
	}
	.row {
		font-size: 0.75rem;
		gap: 0.5rem;
	}
	.priority-high {
		background: var(--danger-soft);
		color: var(--danger);
	}
	.priority-low {
		color: var(--muted);
	}
	.due {
		color: var(--muted);
	}
	.overdue {
		color: var(--danger);
		font-weight: 500;
	}
	.comments {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		color: var(--muted);
	}
	.avatar {
		display: grid;
		place-items: center;
		width: 1.5rem;
		height: 1.5rem;
		margin-left: auto;
		border-radius: 50%;
		background: var(--accent-soft);
		color: var(--accent-text);
		font-size: 0.625rem;
		font-weight: 700;
	}
	.timer {
		flex: none;
		width: 2rem;
		min-height: 2rem;
		padding: 0;
		border-color: transparent;
		background: var(--accent-soft);
		color: var(--accent-text);
	}
	.avatar + .timer {
		margin-left: 0;
	}
	:not(.avatar) + .timer {
		margin-left: auto;
	}
	.timer.on {
		background: var(--accent);
		color: var(--accent-fg);
	}
	.status {
		display: none;
		align-self: flex-start;
		width: auto;
		min-height: 34px;
		padding: 0.25rem 0.5rem;
		font-size: 0.8125rem;
	}
	@media (max-width: 719px) {
		.status {
			display: block;
		}
	}
</style>
