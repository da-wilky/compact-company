<script lang="ts">
	import { flip } from 'svelte/animate';
	import type { Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import TodoCard from './TodoCard.svelte';

	type Todo = Schemas['Todo'];
	let {
		statuses,
		todos,
		onmove
	}: {
		statuses: Schemas['Status'][];
		todos: Todo[];
		onmove: (todo: Todo, statusId: number, beforeId: number | null) => void;
	} = $props();

	const PEEK = 22;
	const GAP = 12;
	const EDGE = 44;
	const DWELL = 550;
	const LONG_PRESS = 350;
	const sides = ['left', 'right'] as const;

	const columns = $derived(
		statuses.map((status) => ({ status, todos: todos.filter((t) => t.status_id === status.id) }))
	);

	let width = $state(0);
	let narrow = $state(false);
	let index = $state(0);
	let swipe = $state(0);
	let swiping = $state(false);
	const step = $derived(width - 2 * PEEK + GAP);
	const offset = $derived(narrow ? PEEK - index * step + swipe : 0);

	$effect(() => {
		const query = matchMedia('(max-width: 719px)');
		narrow = query.matches;
		const change = () => (narrow = query.matches);
		query.addEventListener('change', change);
		return () => query.removeEventListener('change', change);
	});

	$effect(() => {
		if (index > columns.length - 1) index = Math.max(0, columns.length - 1);
	});

	type Target = { statusId: number; beforeId: number | null };
	let drag = $state<{
		todo: Todo;
		x: number;
		y: number;
		dx: number;
		dy: number;
		width: number;
		target: Target | null;
		edge: 'left' | 'right' | null;
	} | null>(null);

	let press: {
		todo: Todo | null;
		card: HTMLElement | null;
		x: number;
		y: number;
		touch: boolean;
		timer?: ReturnType<typeof setTimeout>;
		mode: 'pending' | 'swipe' | 'drag' | 'done';
	} | null = null;
	let dwell: ReturnType<typeof setTimeout> | undefined;
	let suppressClick = false;

	function down(event: PointerEvent) {
		if (event.button !== 0 || (event.target as HTMLElement).closest('select, button')) return;
		const card = (event.target as HTMLElement).closest<HTMLElement>('[data-todo]');
		const todo = card ? todos.find((t) => t.id === Number(card.dataset.todo)) ?? null : null;
		const touch = event.pointerType !== 'mouse';
		press = { todo, card, x: event.clientX, y: event.clientY, touch, mode: 'pending' };
		if (todo && touch) press.timer = setTimeout(() => begin(event.clientX, event.clientY), LONG_PRESS);
		addEventListener('pointermove', move);
		addEventListener('pointerup', up);
		addEventListener('pointercancel', cancel);
		addEventListener('touchmove', holdStill, { passive: false });
	}

	function begin(x: number, y: number) {
		if (!press?.todo || !press.card || press.mode !== 'pending') return;
		const rect = press.card.getBoundingClientRect();
		press.mode = 'drag';
		drag = {
			todo: press.todo,
			x,
			y,
			dx: x - rect.left,
			dy: y - rect.top,
			width: rect.width,
			target: null,
			edge: null
		};
		navigator.vibrate?.(10);
		aim(x, y);
	}

	function move(event: PointerEvent) {
		if (!press) return;
		const dx = event.clientX - press.x;
		const dy = event.clientY - press.y;
		if (press.mode === 'pending') {
			const far = Math.hypot(dx, dy) > (press.touch ? 8 : 5);
			if (!far) return;
			clearTimeout(press.timer);
			if (press.touch) {
				press.mode = narrow && Math.abs(dx) > Math.abs(dy) ? 'swipe' : 'done';
			} else if (press.todo) {
				begin(press.x, press.y);
			} else {
				press.mode = 'done';
			}
		}
		if (press.mode === 'swipe') {
			swiping = true;
			const atEnd = (index === 0 && dx > 0) || (index === columns.length - 1 && dx < 0);
			swipe = atEnd ? dx / 3 : dx;
		} else if (press.mode === 'drag' && drag) {
			drag.x = event.clientX;
			drag.y = event.clientY;
			aim(event.clientX, event.clientY);
		}
	}

	function aim(x: number, y: number) {
		if (!drag) return;
		const edge = narrow ? (x < EDGE ? 'left' : x > innerWidth - EDGE ? 'right' : null) : null;
		if (edge !== drag.edge) {
			drag.edge = edge;
			clearTimeout(dwell);
			if (edge) dwell = setTimeout(turn, DWELL);
		}
		const column = document
			.elementsFromPoint(x, y)
			.find((el) => (el as HTMLElement).dataset?.status) as HTMLElement | undefined;
		if (!column) return;
		const statusId = Number(column.dataset.status);
		const cards = [...column.querySelectorAll<HTMLElement>('[data-todo]')].filter(
			(card) => Number(card.dataset.todo) !== drag?.todo.id
		);
		const before = cards.find((card) => {
			const rect = card.getBoundingClientRect();
			return y < rect.top + rect.height / 2;
		});
		drag.target = { statusId, beforeId: before ? Number(before.dataset.todo) : null };
	}

	// holding a card at the screen edge turns the board to the neighbouring status
	function turn() {
		if (!drag?.edge) return;
		const next = index + (drag.edge === 'left' ? -1 : 1);
		if (next < 0 || next >= columns.length) return;
		index = next;
		drag.target = { statusId: columns[next].status.id, beforeId: null };
		dwell = setTimeout(turn, DWELL * 1.6);
	}

	function up() {
		if (press?.mode === 'swipe') {
			if (Math.abs(swipe) > Math.min(80, step / 4)) {
				index = Math.min(columns.length - 1, Math.max(0, index + (swipe < 0 ? 1 : -1)));
			}
		} else if (press?.mode === 'drag' && drag?.target) {
			const { todo, target } = drag;
			const siblings = columns.find((c) => c.status.id === todo.status_id)?.todos ?? [];
			const next = siblings[siblings.findIndex((t) => t.id === todo.id) + 1]?.id ?? null;
			const unchanged = target.statusId === todo.status_id && target.beforeId === next;
			if (!unchanged) onmove(todo, target.statusId, target.beforeId);
			suppressClick = true;
		}
		finish();
	}

	function cancel() {
		finish();
	}

	function finish() {
		clearTimeout(press?.timer);
		clearTimeout(dwell);
		press = null;
		drag = null;
		swipe = 0;
		swiping = false;
		removeEventListener('pointermove', move);
		removeEventListener('pointerup', up);
		removeEventListener('pointercancel', cancel);
		removeEventListener('touchmove', holdStill);
	}

	function holdStill(event: TouchEvent) {
		if (press?.mode === 'drag' || press?.mode === 'swipe') event.preventDefault();
	}

	function click(event: MouseEvent) {
		if (suppressClick) {
			event.preventDefault();
			event.stopPropagation();
			suppressClick = false;
		}
	}
</script>

{#if narrow}
	<div class="steps">
		{#each columns as column, i (column.status.id)}
			<button
				class:current={i === index}
				aria-current={i === index ? 'true' : undefined}
				onclick={() => (index = i)}
			>
				<span class="bar"></span>
				<span class="name">{column.status.name} {column.todos.length}</span>
			</button>
		{/each}
	</div>
{/if}

<div class="viewport" bind:clientWidth={width} class:narrow>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="track"
		class:animate={!swiping && !drag}
		style:transform={narrow ? `translateX(${offset}px)` : undefined}
		style:--column={narrow ? `${width - 2 * PEEK}px` : undefined}
		onpointerdown={down}
		onclickcapture={click}
		oncontextmenu={(e) => press?.todo && e.preventDefault()}
	>
		{#each columns as column (column.status.id)}
			<section
				class="column"
				data-status={column.status.id}
				class:target={drag?.target?.statusId === column.status.id}
				aria-label={column.status.name}
			>
				<h2>
					{column.status.name}<span class="muted">{column.todos.length}</span>
				</h2>
				<div class="cards">
					{#each column.todos as todo (todo.id)}
						<div animate:flip={{ duration: 180 }}>
							{#if drag?.target?.statusId === column.status.id && drag.target.beforeId === todo.id}
								<div class="indicator"></div>
							{/if}
							<TodoCard
								{todo}
								{statuses}
								lifted={drag?.todo.id === todo.id}
								onstatus={(statusId) => onmove(todo, statusId, null)}
							/>
						</div>
					{/each}
					{#if drag?.target?.statusId === column.status.id && drag.target.beforeId === null}
						<div class="indicator"></div>
					{/if}
					{#if !column.todos.length}<p class="empty-column">{t('todos.column_empty')}</p>{/if}
				</div>
			</section>
		{/each}
	</div>

	{#if drag && narrow}
		{#each sides as side (side)}
			{@const next = columns[index + (side === 'left' ? -1 : 1)]}
			{#if next}
				<div class="edge {side}" class:active={drag.edge === side}>
					<span>{next.status.name}</span>
				</div>
			{/if}
		{/each}
	{/if}
</div>

{#if drag}
	<div
		class="ghost"
		style:left="{drag.x - drag.dx}px"
		style:top="{drag.y - drag.dy}px"
		style:width="{drag.width}px"
	>
		<TodoCard todo={drag.todo} {statuses} />
	</div>
{/if}

<style>
	.viewport {
		position: relative;
		overflow-x: auto;
		padding-bottom: 0.5rem;
	}
	.viewport.narrow {
		overflow: hidden;
		margin: 0 -1rem;
	}
	.track {
		display: grid;
		grid-auto-flow: column;
		grid-auto-columns: minmax(13rem, 1fr);
		gap: 1rem;
		touch-action: pan-y;
	}
	.narrow .track {
		display: flex;
		gap: 12px;
	}
	.narrow .column {
		flex: 0 0 var(--column);
	}
	.track.animate {
		transition: transform 0.25s ease;
	}
	.column {
		display: flex;
		flex-direction: column;
		gap: 0.625rem;
		min-height: 24rem;
		padding: 0.75rem;
		border: 1px solid transparent;
		border-radius: var(--radius-lg);
		background: var(--surface-2);
		transition: border-color 0.15s;
	}
	.column.target {
		border-color: var(--accent-soft-border);
	}
	h2 {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin: 0 0.25rem;
		font-size: 0.9375rem;
	}
	h2 .muted {
		font-weight: 500;
	}
	.cards {
		display: flex;
		flex-direction: column;
		gap: 0.625rem;
		flex: 1;
	}
	.indicator {
		height: 3px;
		margin: -0.375rem 0 0.25rem;
		border-radius: 2px;
		background: var(--accent);
	}
	.empty-column {
		padding: 1rem 0.5rem;
		color: var(--muted);
		font-size: 0.8125rem;
		text-align: center;
	}
	.ghost {
		position: fixed;
		z-index: 20;
		pointer-events: none;
		rotate: -2deg;
		filter: drop-shadow(var(--shadow-raised));
	}
	.steps {
		display: grid;
		grid-auto-columns: minmax(0, 1fr);
		grid-auto-flow: column;
		gap: 0.375rem;
		margin-bottom: 0.75rem;
	}
	.steps button {
		display: flex;
		flex-direction: column;
		align-items: stretch;
		gap: 0.375rem;
		min-height: 0;
		padding: 0.25rem 0;
		border: 0;
		background: none;
		color: var(--muted);
		font-size: 0.75rem;
		font-weight: 400;
		white-space: normal;
		text-align: left;
	}
	.steps .bar {
		height: 4px;
		border-radius: 2px;
		background: var(--border);
	}
	.steps .current {
		color: var(--accent-text);
		font-weight: 600;
	}
	.steps .current .bar {
		background: var(--accent);
	}
	.edge {
		position: absolute;
		top: 0;
		bottom: 0;
		z-index: 15;
		display: flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		background: var(--accent-soft);
		color: var(--accent-text);
		font-size: 0.75rem;
		font-weight: 600;
		transition: width 0.15s;
		pointer-events: none;
	}
	.edge span {
		writing-mode: vertical-rl;
	}
	.edge.left {
		left: 0;
		border-radius: 0 var(--radius-lg) var(--radius-lg) 0;
	}
	.edge.right {
		right: 0;
		border-radius: var(--radius-lg) 0 0 var(--radius-lg);
	}
	.edge.left span {
		rotate: 180deg;
	}
	.edge.active {
		width: 72px;
		background: var(--accent);
		color: var(--accent-fg);
	}
</style>
