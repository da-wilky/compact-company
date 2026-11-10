<script lang="ts">
	import { toasts } from '#lib/toast.svelte.ts';
	import Icon from './Icon.svelte';
</script>

<div role="status" aria-live="polite">
	{#each toasts as toast (toast.id)}
		<p><Icon name="check" size={18} />{toast.text}</p>
	{/each}
</div>

<style>
	div {
		position: fixed;
		z-index: 10;
		left: 50%;
		bottom: calc(var(--bottom-bar) + 1rem + env(safe-area-inset-bottom));
		translate: -50% 0;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		pointer-events: none;
	}
	@media (min-width: 960px) {
		div {
			bottom: 1.5rem;
		}
	}
	p {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.625rem 1rem;
		border-radius: var(--radius);
		background: var(--text);
		color: var(--bg);
		font-weight: 500;
		box-shadow: var(--shadow-raised);
	}
</style>
