<script lang="ts">
	import Hint from '#lib/components/Hint.svelte';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { formatDay, isoDay } from '#lib/dates.ts';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Part = Schemas['ProcessPart'];
	const parts: Part[] = ['general', 'user', 'technical', 'operations'];

	let view = $state<Schemas['ProcessDocumentation']>();
	let answers = $state<Record<string, string>>({});
	let error = $state<ApiError>();
	let saving = $state(false);
	const owner = $derived(session.view?.user?.role === 'owner');

	const day = (iso: string) => formatDay(iso, { dateStyle: 'medium' });
	const language = $derived(i18n.locale === 'de' ? 'de' : 'en');

	function take(data: Schemas['ProcessDocumentation']) {
		view = data;
		answers = Object.fromEntries(data.questions.map((q) => [q.key, q.answer ?? '']));
	}

	async function load() {
		const result = await call(api.GET('/api/process-documentation', { params: { query: { language } } }));
		error = result.error;
		if (result.data) take(result.data);
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saving = true;
		const result = await call(
			api.PUT('/api/process-documentation/answers', { params: { query: { language } }, body: { answers } })
		);
		saving = false;
		error = result.error;
		if (result.data) {
			take(result.data);
			toast(t('process.saved'));
		}
	}

	$effect(() => {
		language;
		load();
	});
</script>

<div class="page-head">
	<div>
		<h1>{t('term.process_documentation')}<Hint term="process_documentation" /></h1>
		<p class="muted">{t('process.intro')}</p>
	</div>
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if view}
	<div class="layout">
		<section class="card stack versions">
			<h2>{t('process.versions')}</h2>
			{#if view.versions.length}
				<ul class="list">
					{#each view.versions as version (version.id)}
						<li>
							<div>
								<strong>{t('process.version', { id: version.id })}</strong>
								<small class="muted">
									{version.valid_until
										? t('process.valid_range', { from: day(version.valid_from), until: day(version.valid_until) })
										: t('process.valid_from', { from: day(version.valid_from) })}
								</small>
								<small class="muted">{version.changes.length ? version.changes.join(' · ') : t('process.first')}</small>
							</div>
							<div class="downloads">
								<a class="button ghost" href="/api/process-documentation/versions/{version.id}/pdf?language=de" download>
									<Icon name="pdf" size={16} />{t('process.german')}
								</a>
								<a class="button ghost" href="/api/process-documentation/versions/{version.id}/pdf?language=en" download>
									<Icon name="pdf" size={16} />{t('process.english')}
								</a>
							</div>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="muted">{t('process.no_versions')}</p>
			{/if}
		</section>

		<form class="stack" onsubmit={save}>
			{#each parts as part (part)}
				{@const questions = view.questions.filter((q) => q.part === part)}
				{#if questions.length}
					<section class="card stack">
						<h2>{t(`process.part.${part}`)}</h2>
						{#each questions as question (question.key)}
							<div class="question stack">
								<label for="answer-{question.key}"><strong>{question.question}</strong></label>
								<p class="muted">{question.help}</p>
								<textarea id="answer-{question.key}" rows="4" readonly={!owner} bind:value={answers[question.key]}
								></textarea>
								<div class="row">
									{#if question.updated_at}
										<small class="muted">{t('process.updated', { date: day(isoDay(new Date(question.updated_at * 1000))) })}</small>
									{/if}
									{#if owner && answers[question.key] !== question.suggestion}
										<button type="button" class="ghost" title={question.suggestion} onclick={() => (answers[question.key] = question.suggestion)}>
											{t('process.use_suggestion')}
										</button>
									{/if}
								</div>
								{#if !answers[question.key]}
									<p class="suggestion"><span class="muted">{t('process.suggestion')}:</span> {question.suggestion}</p>
								{/if}
							</div>
						{/each}
					</section>
				{/if}
			{/each}
			{#if owner}
				<div><button class="primary" disabled={saving}>{t('process.save')}</button></div>
			{/if}
		</form>
	</div>
{/if}

<style>
	.layout {
		display: grid;
		gap: 1rem;
		align-items: start;
	}
	@media (min-width: 1000px) {
		.layout {
			grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
		}
		.versions {
			order: 2;
			position: sticky;
			top: 1rem;
		}
	}
	.versions li {
		flex-wrap: wrap;
	}
	.versions li > div:first-child {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.downloads {
		display: flex;
		gap: 0.25rem;
	}
	.question + .question {
		border-top: 1px solid var(--border);
		padding-top: 1rem;
	}
	.question p {
		margin: 0;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
	}
	.suggestion {
		font-size: 0.875rem;
	}
</style>
