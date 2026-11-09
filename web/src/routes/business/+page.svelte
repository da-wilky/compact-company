<script lang="ts">
	import { api, networkError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { t } from '#lib/i18n.svelte.ts';

	let values = $state<Schemas['Business']>({});
	let pinned = $state<string[]>([]);
	let error = $state<ApiError>();
	let saved = $state(false);

	function show(view: Schemas['BusinessView']) {
		values = view.values;
		pinned = view.pinned;
	}

	async function load() {
		try {
			const { data, error: e } = await api.GET('/api/business');
			if (data) show(data);
			error = e;
		} catch {
			error = networkError;
		}
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saved = false;
		try {
			const { data, error: e } = await api.PUT('/api/business', { body: values });
			if (data) show(data);
			error = e;
			saved = !e;
		} catch {
			error = networkError;
		}
	}

	load();

	const note = (key: string) => (pinned.includes(key) ? t('business.pinned') : undefined);
	const texts = ['name', 'tax_number', 'vat_id'] as const;
</script>

<h1>{t('business.title')}</h1>

<form onsubmit={save}>
	{#each texts as key (key)}
		<Field label={t(`business.${key}`)} error={error?.fields?.[key]} note={note(key)}>
			<input bind:value={values[key]} readonly={pinned.includes(key)} />
		</Field>
	{/each}
	<Field label={t('business.address')} error={error?.fields?.address} note={note('address')}>
		<textarea rows="3" bind:value={values.address} readonly={pinned.includes('address')}
		></textarea>
	</Field>
	<Field label={t('business.small_business')} note={note('small_business')}>
		<input
			type="checkbox"
			bind:checked={values.small_business}
			disabled={pinned.includes('small_business')}
		/>
	</Field>

	{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}
	{#if saved}<p role="status">{t('app.saved')}</p>{/if}
	<button>{t('app.save')}</button>
</form>

<style>
	input:not([type='checkbox']),
	textarea {
		font: inherit;
		padding: 0.4rem;
		border: 1px solid #bbb;
		border-radius: 4px;
	}
	.error {
		color: #b00020;
	}
</style>
