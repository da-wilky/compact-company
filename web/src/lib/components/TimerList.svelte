<script lang="ts">
	import { t } from '#lib/i18n.svelte.ts';
	import { clock, entryTitle, openTimer, stopTimer, timers } from '#lib/timers.svelte.ts';
	import Icon from './Icon.svelte';

	let { large = false }: { large?: boolean } = $props();

	const total = $derived(timers.list.reduce((sum, e) => sum + (timers.now - e.started_at), 0));
</script>

<section class="timers" class:large aria-label={t('time.running_timers')}>
	{#if timers.list.length}
		<header>
			<span>{t('time.timers_running', { count: timers.list.length })}</span>
			<span class="clock">{clock(Math.max(0, total), true)}</span>
		</header>
		{#each timers.list as entry (entry.id)}
			<div class="timer">
				<span class="dot" aria-hidden="true"></span>
				<div>
					<span class="title">{entryTitle(entry)}</span>
					<small>{clock(Math.max(0, timers.now - entry.started_at), true)} · {entry.customer_name}</small>
				</div>
				<button class="icon stop" onclick={() => stopTimer(entry.id)} aria-label={t('time.stop')} title={t('time.stop')}>
					<Icon name="stop" size={large ? 16 : 12} />
				</button>
			</div>
		{/each}
	{/if}
	<button class="start" onclick={() => openTimer()}>
		<Icon name="play" size={14} />{t(timers.list.length ? 'time.start_another' : 'time.start')}
	</button>
</section>

<style>
	.timers {
		display: flex;
		flex-direction: column;
		gap: 0.625rem;
		padding: 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--bg);
	}
	.large {
		padding: 0;
		border: 0;
		background: none;
		gap: 0.75rem;
	}
	header {
		display: flex;
		align-items: baseline;
		color: var(--muted);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.clock {
		margin-left: auto;
		color: var(--text);
		font: 600 0.9375rem var(--font-display);
		font-variant-numeric: tabular-nums;
	}
	.large .clock {
		font-size: 1.125rem;
	}
	.timer {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.large .timer {
		padding: 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--bg);
	}
	.dot {
		flex: none;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--accent);
		animation: pulse 2s ease-in-out infinite;
	}
	@keyframes pulse {
		50% {
			opacity: 0.35;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.dot {
			animation: none;
		}
	}
	.timer > div {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.large .title {
		font-size: 0.875rem;
	}
	small {
		color: var(--muted);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
	}
	.stop {
		flex: none;
		width: 2rem;
		min-height: 2rem;
	}
	.large .stop {
		width: var(--control);
		min-height: var(--control);
		border-color: var(--accent);
		background: var(--accent);
		color: var(--accent-fg);
	}
	.start {
		min-height: 2.125rem;
		border-style: dashed;
		background: transparent;
		font-size: 0.8125rem;
	}
	.large .start {
		min-height: var(--control);
	}
</style>
