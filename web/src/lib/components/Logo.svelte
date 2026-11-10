<script lang="ts">
	let { size = 32, wordmark = true }: { size?: number; wordmark?: boolean } = $props();
</script>

<span class="logo">
	<svg aria-hidden="true" width={size} height={size} viewBox="0 0 32 32">
		<rect width="32" height="32" rx="9" fill="var(--accent)" />
		<path
			d="M22.5 10.5A8.5 8.5 0 1 0 22.5 21.5"
			fill="none"
			stroke="var(--accent-fg)"
			stroke-width="3"
			stroke-linecap="round"
		/>
		<path
			d="M19 13.4A4 4 0 1 0 19 18.6"
			fill="none"
			stroke="var(--accent-fg)"
			stroke-width="2.4"
			stroke-linecap="round"
		/>
	</svg>
	{#if wordmark}
		<span class="word">compact<br /><span>company</span></span>
	{:else}
		<span class="visually-hidden">compact-company</span>
	{/if}
</span>

<style>
	.logo {
		display: inline-flex;
		align-items: center;
		gap: 0.625rem;
	}
	.word {
		font: 700 0.9375rem/1.05 var(--font-display);
		letter-spacing: 0.01em;
		color: var(--text);
	}
	.word span {
		color: var(--accent-text);
	}
</style>
