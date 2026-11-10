<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	let snippets = $state<Schemas['Snippet'][]>([]);
	let open = $state(false);
	let editing = $state<Schemas['Snippet']>();
	let name = $state('');
	let language = $state<Schemas['Language']>('de');
	let body = $state('');
	let error = $state<ApiError>();

	async function load() {
		const result = await call(api.GET('/api/snippets'));
		snippets = result.data ?? [];
		error = result.error;
	}
	load();

	function edit(snippet?: Schemas['Snippet']) {
		editing = snippet;
		name = snippet?.name ?? '';
		language = snippet?.language ?? 'de';
		body = snippet?.body ?? '';
		error = undefined;
		open = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const input = { name, language, body };
		const result = editing
			? await call(api.PUT('/api/snippets/{id}', { params: { path: { id: editing.id } }, body: input }))
			: await call(api.POST('/api/snippets', { body: input }));
		error = result.error;
		if (result.data) {
			open = false;
			toast(t('app.saved'));
			await load();
		}
	}

	async function remove(snippet: Schemas['Snippet']) {
		if (!confirm(t('snippets.delete_confirm', { name: snippet.name }))) return;
		const result = await call(api.DELETE('/api/snippets/{id}', { params: { path: { id: snippet.id } } }));
		error = result.error;
		await load();
	}
</script>

<div class="page-head">
	<div>
		<h1>{t('snippets.title')}</h1>
		<p class="muted">{t('snippets.intro')}</p>
	</div>
	<button class="primary" onclick={() => edit()}><Icon name="plus" size={18} />{t('snippets.new')}</button>
</div>

<div class="stack">
	{#if error && !open}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if snippets.length}
		<ul class="list card-list">
			{#each snippets as snippet (snippet.id)}
				<li>
					<div class="text">
						<strong>{snippet.name} <small class="badge">{snippet.language.toUpperCase()}</small></strong>
						<small class="muted">{snippet.body}</small>
					</div>
					<button class="ghost" onclick={() => edit(snippet)}>{t('app.edit')}</button>
					<button class="ghost" onclick={() => remove(snippet)}>{t('app.delete')}</button>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="card empty">{t('snippets.none')}</p>
	{/if}
</div>

<Dialog bind:open title={t(editing ? 'snippets.edit' : 'snippets.new')}>
	<form class="stack" onsubmit={save}>
		<Field label={t('snippets.name')} error={error?.fields?.name}>
			<input bind:value={name} />
		</Field>
		<Field label={t('customers.language')}>
			<select bind:value={language}>
				<option value="de">{t('language.de')}</option>
				<option value="en">{t('language.en')}</option>
			</select>
		</Field>
		<Field label={t('snippets.body')} error={error?.fields?.body}>
			<textarea rows="6" bind:value={body}></textarea>
		</Field>
		{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
		<div><button class="primary">{t('app.save')}</button></div>
	</form>
</Dialog>

<style>
	.card-list {
		max-width: 48rem;
		background: var(--surface);
	}
	.text {
		display: grid;
		min-width: 0;
	}
	.text small {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
