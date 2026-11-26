<script lang="ts">
	import { guides, terms } from '#lib/help.svelte.ts';
	import { has, t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';

	const role = $derived(session.view?.user?.role);
	const mine = $derived(guides.filter((guide) => role && guide.roles.includes(role)));
</script>

<div class="page-head">
	<div>
		<h1>{t('help.title')}</h1>
		<p class="muted intro">{t('help.intro')}</p>
	</div>
</div>

<h2>{t('help.guides')}</h2>
<div class="guides">
	{#each mine as guide (guide.key)}
		<section class="card stack" id="guide-{guide.key}">
			<h3>{t(`help.guide.${guide.key}.title`)}</h3>
			<ol>
				{#each guide.links as link, i (i)}
					<li>
						{t(`help.guide.${guide.key}.${i + 1}`)}
						{#if link}<a href={link}>{t('help.open')}</a>{/if}
					</li>
				{/each}
			</ol>
		</section>
	{/each}
</div>

<h2>{t('help.terms')}</h2>
<dl class="card">
	{#each terms as term (term)}
		<div id={term}>
			<dt>{t(`help.${term}.name`)}</dt>
			<dd>
				<p>{t(`help.${term}.text`)}</p>
				{#if has(`help.${term}.law`)}<small class="muted">{t('help.law', { law: t(`help.${term}.law`) })}</small>{/if}
			</dd>
		</div>
	{/each}
</dl>

<style>
	.intro {
		max-width: 48rem;
	}
	h2 {
		margin: 1.5rem 0 0.75rem;
	}
	.guides {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 22rem), 1fr));
		gap: 1rem;
		align-items: start;
	}
	ol {
		display: grid;
		gap: 0.5rem;
		margin: 0;
		padding-inline-start: 1.25rem;
	}
	dl {
		display: grid;
		margin: 0;
		padding: 0;
	}
	dl > div {
		display: grid;
		gap: 0.25rem;
		padding: 1rem 1.25rem;
		border-top: 1px solid var(--border);
		scroll-margin-top: 1rem;
	}
	dl > div:first-child {
		border-top: 0;
	}
	dl > div:target {
		background: var(--accent-soft);
	}
	@media (min-width: 900px) {
		dl > div {
			grid-template-columns: 14rem minmax(0, 1fr);
			gap: 1.5rem;
		}
	}
	dt {
		font-weight: 600;
	}
	dd {
		display: grid;
		gap: 0.25rem;
		margin: 0;
		max-width: 48rem;
	}
	dd p {
		margin: 0;
	}
</style>
