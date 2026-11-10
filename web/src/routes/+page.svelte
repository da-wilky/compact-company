<script lang="ts">
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
</script>

<div class="page-head">
	<div>
		<p class="muted">{t('nav.overview')}</p>
		<h1>{t('home.greeting', { name: session.view?.user?.name ?? '' })}</h1>
	</div>
</div>

<section class="card empty">
	<p>{t('home.empty')}</p>
</section>
